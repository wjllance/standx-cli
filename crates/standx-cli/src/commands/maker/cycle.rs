use super::ledger::{adopt_order, apply_rest_trade};
#[cfg(test)]
use super::ledger::{apply_account_trade, apply_order_update, maker_trade_fill};
use super::model::{position_for_symbol, rest_order_observation};
use super::output::{
    emit_cycle_skip, emit_maker_cycle, log_maker_event, CycleOutput, MakerLogEvent,
};
use super::pipeline::{fetch_account_audit, CycleRequest, CycleResult, CycleState};
use super::recovery::PositionReconciliationError;
use anyhow::Result;
use standx_maker::{
    self as maker, AccountProjectionEvent, MakerAccountProjection, MakerFill, MakerLedger,
    MakerStats, ProjectionPendingCancel, ProjectionPendingPlace, ProjectionRegistryError,
    RestingQuote, MAX_PENDING_ORDER_REQUESTS,
};
use standx_sdk::account_stream::AccountStreamHealth;
#[cfg(test)]
use standx_sdk::account_stream::{OrderUpdate, TradeUpdate};
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::models::{Balance, OrderSide, OrderType, TimeInForce, Trade};
use standx_sdk::order_response::{OrderCommandSender, OrderResponseHealth};
use std::time::Duration;

fn collect_current_run_fills(
    trades: Vec<Trade>,
    ledger: &mut MakerLedger,
    session_started_at: i64,
    now: i64,
    mark: f64,
    stats: &mut MakerStats,
    fills: &mut Vec<MakerFill>,
) -> Result<bool> {
    let mut exit_fill_observed = false;
    for trade in trades {
        exit_fill_observed |=
            apply_rest_trade(ledger, trade, session_started_at, now, mark, stats, fills)?;
    }
    Ok(exit_fill_observed)
}

fn unhealthy_order_response(health: Option<&OrderResponseHealth>) -> Option<String> {
    match health {
        Some(health) if health.is_healthy() => None,
        Some(health) => Some(health.failure_reason().unwrap_or_else(|| {
            "order-response stream became unhealthy without a recorded reason".to_string()
        })),
        None => Some("order-response health state is unavailable".to_string()),
    }
}

fn unhealthy_account_stream(health: Option<&AccountStreamHealth>) -> Option<String> {
    match health {
        Some(health) if health.is_healthy() => None,
        Some(health) => Some(health.failure_reason().unwrap_or_else(|| {
            "account stream became unhealthy without a recorded reason".to_string()
        })),
        None => Some("account stream health state is unavailable".to_string()),
    }
}

fn ensure_live_streams_healthy(
    account_health: Option<&AccountStreamHealth>,
    order_health: Option<&OrderResponseHealth>,
) -> Result<()> {
    if let Some(reason) = unhealthy_account_stream(account_health) {
        return Err(anyhow::anyhow!(
            "{reason}; refusing further live order actions"
        ));
    }
    if let Some(reason) = unhealthy_order_response(order_health) {
        return Err(anyhow::anyhow!(
            "{reason}; refusing further live order actions"
        ));
    }
    Ok(())
}

fn apply_request_submission(
    projection: &mut MakerAccountProjection,
    event: AccountProjectionEvent,
) -> Result<()> {
    let generation = projection.generation();
    let outcome = projection.apply(generation, event);
    match outcome.request_registry_error {
        Some(error) => Err(anyhow::Error::new(error)),
        None => Ok(()),
    }
}

fn ensure_request_registry_capacity(projection: Option<&MakerAccountProjection>) -> Result<()> {
    let projection =
        projection.ok_or_else(|| anyhow::anyhow!("live maker request registry is unavailable"))?;
    if projection.pending_request_count() >= MAX_PENDING_ORDER_REQUESTS {
        return Err(anyhow::Error::new(ProjectionRegistryError::Capacity {
            limit: MAX_PENDING_ORDER_REQUESTS,
        }));
    }
    Ok(())
}

fn live_order_commands(commands: Option<&OrderCommandSender>) -> Result<&OrderCommandSender> {
    commands.ok_or_else(|| anyhow::anyhow!("order-command stream is unavailable"))
}

/// One reconcile cycle over an already-acquired market snapshot.
/// Returns (places, cancels, holds, fills) counts. `sim_position` carries the
/// paper-mode simulated inventory across cycles (unused in live).
pub(super) async fn maker_cycle(
    request: CycleRequest<'_>,
    state: CycleState<'_>,
) -> Result<CycleResult> {
    let CycleRequest {
        client,
        symbol,
        cfg,
        live,
        cycle,
        mark,
        best_bid,
        best_ask,
        market_source,
        market_fallback_reason,
        max_divergence_bps,
        inventory_exit_pct,
        inventory_exit_qty,
        session_started_at,
        run_order_prefix,
        starting_position,
        output_format,
        order_commands,
        order_response_health,
        account_stream_health,
    } = request;
    let CycleState {
        resting,
        mut account_projection,
        inventory_exit_pending,
        ledger,
        sim_position,
        stats,
        breaker,
        live_account_poll,
    } = state;
    use maker::{format_decimals, paper_quote_filled, Action, CycleInput, MarketSnapshot};

    // 0. Run all market-only guards before any account/order I/O. The pure
    // planner owns breaker observation and data-consistency policy; this
    // adapter only renders the resulting skip decision.
    let market = MarketSnapshot {
        mark,
        best_bid,
        best_ask,
    };
    let preflight = maker::preflight_cycle(breaker, market, max_divergence_bps, live);
    let halted = match preflight.skip {
        Some(skip) => {
            emit_cycle_skip(
                output_format,
                cycle,
                symbol,
                live,
                mark,
                cfg.price_decimals,
                max_divergence_bps,
                skip,
            );
            return Ok(CycleResult::default());
        }
        None => preflight.halted,
    };

    // 2. Use the authenticated account-stream projection in live mode or the
    //    simulated in-memory book in paper mode. REST is only a periodic audit.
    let position: f64;
    let mut projected_resting = Vec::new();
    let mut account_balance: Option<Balance> = None;
    let mut fills: Vec<MakerFill> = Vec::new();
    let mut exit_fill_observed = false;
    if live {
        let projection = account_projection
            .as_deref_mut()
            .expect("live maker cycles require initialized account projection");
        let generation = projection.generation();
        projection.apply(generation, AccountProjectionEvent::AdvanceCycle { cycle });
        let poll =
            live_account_poll.expect("live maker cycles require initialized account polling state");
        let poll_now = std::time::Instant::now();
        let now = chrono::Utc::now().timestamp();
        let audit_due = poll.account_audit_due(poll_now);
        let balance_refresh_due = poll.balance_refresh_due(poll_now);
        let audit_future = async {
            if audit_due {
                Some(fetch_account_audit(client, symbol, session_started_at, now).await)
            } else {
                None
            }
        };
        let balance_future = async {
            if balance_refresh_due {
                Some(client.get_balance().await)
            } else {
                None
            }
        };
        let (audit, refreshed_balance) = tokio::join!(audit_future, balance_future);
        // Resolve every due read before mutating the current-run ledger. A
        // failed audit must leave this cycle's accounting exactly untouched.
        let audit = match audit {
            Some(audit) => Some(audit?),
            None => None,
        };
        if let Some(refreshed_balance) = refreshed_balance {
            match refreshed_balance {
                Ok(balance) => poll.record_balance_refresh(balance, poll_now),
                Err(error) => {
                    poll.record_balance_refresh_failure(poll_now);
                    if !poll.balance_is_within_stale_limit(poll_now) {
                        return Err(error.into());
                    }
                    eprintln!(
                        "⚠️  account balance refresh failed; reusing cached balance for up to 60s: {error}"
                    );
                }
            }
        }
        account_balance = Some(poll.balance().clone());

        if let Some(audit) = audit {
            for order in audit.open_orders.iter().chain(audit.filled_orders.iter()) {
                adopt_order(ledger, order, run_order_prefix)?;
            }
            let fill_start = fills.len();
            exit_fill_observed |= collect_current_run_fills(
                audit.trades,
                ledger,
                session_started_at,
                now,
                mark,
                stats,
                &mut fills,
            )?;
            for fill in &fills[fill_start..] {
                if let Some(order_id) = fill.order_id {
                    projection.apply(
                        generation,
                        AccountProjectionEvent::TradeApplied {
                            order_id,
                            qty: fill.qty,
                        },
                    );
                }
            }
            let observed_position = position_for_symbol(&audit.positions, symbol)?;
            let observations = audit
                .open_orders
                .iter()
                .map(rest_order_observation)
                .collect::<Result<Vec<_>>>()?;
            let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
            if (observed_position - ledger.expected_position).abs() > qty_tolerance {
                // Preserve the prior bounded anomaly window. The normal path
                // performs no REST read; this sleep is reached only by a
                // periodic audit that found an unexplained position gap.
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Err(anyhow::Error::new(
                    PositionReconciliationError::position_mismatch(
                        ledger.expected_position,
                        observed_position,
                    ),
                ));
            }
            if let Err(error) = projection.verify_rest_snapshot(
                generation,
                observed_position,
                &observations,
                qty_tolerance,
            ) {
                eprintln!("⚠️  account projection audit failed: {error}");
                // Reuse the runtime's immediate reconciliation/freeze path;
                // the detailed order/projection mismatch is emitted above.
                return Err(anyhow::Error::new(
                    PositionReconciliationError::account_projection_mismatch(
                        ledger.expected_position,
                        observed_position,
                        error.to_string(),
                    ),
                ));
            }
            projection.apply(
                generation,
                AccountProjectionEvent::PositionObserved {
                    position: observed_position,
                },
            );
            poll.record_account_audit(poll_now);
        }
        position = ledger.expected_position;
        projected_resting = projection.resting_quotes();
    } else {
        // Paper mode: simulate fills against the touch so inventory (and thus
        // skew) is observable without going live. A crossed resting quote is
        // taken off the book and its signed qty folded into the position; the
        // reconcile below then re-quotes the vacated level.
        let mut i = 0;
        while i < resting.len() {
            if paper_quote_filled(resting[i].side, resting[i].price, best_bid, best_ask) {
                let q = resting.remove(i);
                *sim_position += match q.side {
                    OrderSide::Buy => q.qty,
                    OrderSide::Sell => -q.qty,
                };
                stats.record_fill(q.side, q.price, q.qty, mark);
                fills.push(MakerFill {
                    side: q.side,
                    price: q.price,
                    qty: q.qty,
                    trade_id: None,
                    order_id: None,
                    trade_ts: None,
                    origin: "paper",
                });
            } else {
                i += 1;
            }
        }
        position = *sim_position;
    }

    // 3. Build the pure quote/exit plan from the synchronized state.
    let active_resting = if live {
        projected_resting.as_slice()
    } else {
        resting.as_slice()
    };
    let pending_slots = account_projection
        .as_deref()
        .map(|projection| projection.pending_places())
        .unwrap_or_default()
        .iter()
        .map(|place| (place.side, place.level))
        .collect::<Vec<_>>();
    let plan = maker::plan_cycle(
        cfg,
        CycleInput {
            cycle,
            market,
            position,
            resting: active_resting,
            pending_slots: &pending_slots,
            active_exit_enabled: live,
            inventory_exit_pct,
            inventory_exit_qty,
        },
        halted,
    );
    let raw_inventory_exit = plan.requested_inventory_exit;
    if exit_fill_observed {
        *inventory_exit_pending = false;
    }
    if raw_inventory_exit.is_none() {
        *inventory_exit_pending = false;
    }
    if raw_inventory_exit.is_some() && *inventory_exit_pending {
        return Err(anyhow::anyhow!(
            "inventory exit is still awaiting venue confirmation; refusing to submit another"
        ));
    }

    let inventory_exit = plan.inventory_exit;
    // The pure reconciler intentionally knows nothing about transport state.
    // Remove desired placements whose slots are still reserved by an HTTP
    // submission before both execution and telemetry, so output never claims
    // a duplicate place occurred.
    let actions: Vec<Action> = plan
        .actions
        .into_iter()
        .filter(|action| match action {
            Action::Place(q)
                if live
                    && maker::pending_covers_slot(
                        account_projection
                            .as_deref()
                            .into_iter()
                            .flat_map(|projection| projection.pending_places())
                            .map(|place| maker::QuoteSlot {
                                side: place.side,
                                level: place.level,
                            }),
                        q.side,
                        q.level,
                    ) =>
            {
                log_maker_event(MakerLogEvent {
                    output_format,
                    symbol,
                    cycle,
                    action: "place_pending",
                    side: q.side,
                    level: q.level,
                    price: q.price,
                    price_decimals: cfg.price_decimals,
                    detail: "awaiting asynchronous order confirmation",
                });
                false
            }
            _ => true,
        })
        .collect();

    // The pure planner provides the anti-flicker anchor for new placements.
    let ref_center = plan.ref_center;

    // 4. Execute. A socket-write failure propagates toward the fail-safe;
    // business acceptance/rejection is handled later through the correlated
    // order-response stream.
    let mut places: u64 = 0;
    let mut cancels: u64 = 0;
    let mut holds: u64 = 0;
    for action in &actions {
        match action {
            Action::Cancel {
                order_id,
                side,
                level,
                price,
                ..
            } => {
                if live {
                    ensure_live_streams_healthy(account_stream_health, order_response_health)?;
                    if let Some(id) = order_id {
                        ensure_request_registry_capacity(account_projection.as_deref())?;
                        let order_id = id.parse::<u64>().map_err(|_| {
                            anyhow::anyhow!(
                                "projected maker order has non-integer exchange ID '{id}'"
                            )
                        })?;
                        let commands = live_order_commands(order_commands)?;
                        let command = commands.prepare_cancel_order(id)?;
                        let request_id = command.request_id().to_string();
                        let projection = account_projection
                            .as_deref_mut()
                            .expect("live maker cycles require initialized account projection");
                        apply_request_submission(
                            projection,
                            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                                request_id,
                                order_id,
                                side: *side,
                                level: *level,
                                price: *price,
                                cycle,
                            }),
                        )?;
                        commands.send_prepared(command).await?;
                        cancels += 1;
                    }
                } else {
                    resting.retain(|r| !(r.side == *side && r.level == *level));
                    cancels += 1;
                }
            }
            Action::Place(q) => {
                if live {
                    ensure_live_streams_healthy(account_stream_health, order_response_health)?;
                    ensure_request_registry_capacity(account_projection.as_deref())?;
                    let cl_ord_id =
                        maker::quote_client_order_id(run_order_prefix, cycle, q.side, q.level);
                    let commands = live_order_commands(order_commands)?;
                    let command = commands.prepare_create_order(&CreateOrderParams {
                        symbol: symbol.to_string(),
                        cl_ord_id: Some(cl_ord_id.clone()),
                        side: q.side,
                        order_type: OrderType::Limit,
                        quantity: format_decimals(q.qty, cfg.qty_decimals),
                        price: Some(format_decimals(q.price, cfg.price_decimals)),
                        // Post-only: reject instead of taking if the
                        // price would cross by arrival time.
                        time_in_force: Some(TimeInForce::Alo),
                        reduce_only: false,
                        stop_price: None,
                        sl_price: None,
                        tp_price: None,
                    })?;
                    let request_id = command.request_id().to_string();
                    let projection = account_projection
                        .as_deref_mut()
                        .expect("live maker cycles require initialized account projection");
                    apply_request_submission(
                        projection,
                        AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
                            request_id,
                            client_order_id: cl_ord_id,
                            side: q.side,
                            price: q.price,
                            qty: q.qty,
                            level: q.level,
                            ref_center,
                            cycle,
                        }),
                    )?;
                    commands.send_prepared(command).await?;
                    places += 1;
                } else {
                    resting.push(RestingQuote {
                        order_id: None,
                        side: q.side,
                        level: q.level,
                        price: q.price,
                        qty: q.qty,
                        ref_center,
                        placed_at_cycle: cycle,
                    });
                    places += 1;
                }
            }
            Action::Hold { .. } => holds += 1,
        }
    }

    if let Some(exit) = inventory_exit {
        // Do not race a reduce-only market order against quote cancellations.
        // The next cycle must observe an empty maker book before the single
        // exit request can be submitted.
        let account_clear = account_projection.as_deref().is_some_and(|projection| {
            projection.resting_quotes().is_empty()
                && projection.pending_places().is_empty()
                && projection.pending_cancels().is_empty()
        });
        if account_clear {
            ensure_live_streams_healthy(account_stream_health, order_response_health)?;
            ensure_request_registry_capacity(account_projection.as_deref())?;
            let cl_ord_id = maker::exit_client_order_id(run_order_prefix, cycle);
            let commands = live_order_commands(order_commands)?;
            let command = commands.prepare_create_order(&CreateOrderParams {
                symbol: symbol.to_string(),
                cl_ord_id: Some(cl_ord_id.clone()),
                side: exit.side,
                order_type: OrderType::Market,
                quantity: format_decimals(exit.qty, cfg.qty_decimals),
                price: None,
                time_in_force: None,
                reduce_only: true,
                stop_price: None,
                sl_price: None,
                tp_price: None,
            })?;
            let request_id = command.request_id().to_string();
            // Register the exit submission so its asynchronous ack correlates
            // to a pending entry instead of counting as an unmatched response.
            // The sentinel level keeps it out of quote-slot reservation; a
            // reduce-only market order never rests, so reconciliation drops it
            // when `pending` ages out.
            let projection = account_projection
                .as_deref_mut()
                .expect("live inventory exits require initialized account projection");
            apply_request_submission(
                projection,
                AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
                    request_id,
                    client_order_id: cl_ord_id,
                    side: exit.side,
                    price: mark,
                    qty: exit.qty,
                    level: u32::MAX,
                    ref_center: mark,
                    cycle,
                }),
            )?;
            commands.send_prepared(command).await?;
            *inventory_exit_pending = true;
            log_maker_event(MakerLogEvent {
                output_format,
                symbol,
                cycle,
                action: "inventory_exit_submitted",
                side: exit.side,
                level: 0,
                price: mark,
                price_decimals: cfg.price_decimals,
                detail: "reduce-only market order submitted after maker book cleared",
            });
        }
    }

    // 5. Telemetry uses exact ledger fills in live mode and simulated fills
    // in paper mode; never infer a fill from a position delta.
    let final_resting = if live {
        account_projection
            .as_deref()
            .map(|projection| projection.resting_quotes())
            .unwrap_or_default()
    } else {
        resting.clone()
    };
    let two_sided = final_resting.iter().any(|r| r.side == OrderSide::Buy)
        && final_resting.iter().any(|r| r.side == OrderSide::Sell);
    stats.end_cycle(position, two_sided);

    // 6. Emit.
    emit_maker_cycle(CycleOutput {
        output_format,
        live,
        symbol,
        cycle,
        mark,
        best_bid,
        best_ask,
        market_source,
        market_fallback_reason,
        position,
        starting_position,
        account: account_balance.as_ref(),
        actions: &actions,
        fills: &fills,
        stats,
        halt_vol_bps: halted.then(|| breaker.vol_bps()),
        cfg,
    });

    Ok(CycleResult {
        places,
        cancels,
        holds,
        fills: fills.len() as u64,
        balance: account_balance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(side: Option<&str>, price: &str, qty: &str) -> Trade {
        Trade {
            id: 42,
            time: "2026-07-10T00:00:00Z".to_string(),
            price: price.to_string(),
            qty: qty.to_string(),
            side: side.map(str::to_string),
            is_buyer_taker: false,
            fee_asset: None,
            fee_qty: None,
            pnl: None,
            order_id: Some(7),
            symbol: Some("BTC-USD".to_string()),
            value: None,
        }
    }

    #[test]
    fn maker_trade_fill_requires_complete_venue_fields() {
        assert_eq!(
            maker_trade_fill(&trade(Some("buy"), "99.5", "0.02")).unwrap(),
            (OrderSide::Buy, 99.5, 0.02)
        );
        assert!(maker_trade_fill(&trade(None, "99.5", "0.02"))
            .unwrap_err()
            .to_string()
            .contains("valid side"));
        assert!(maker_trade_fill(&trade(Some("sell"), "bad", "0.02"))
            .unwrap_err()
            .to_string()
            .contains("invalid price"));
    }

    #[test]
    fn current_run_fill_is_recorded_once_with_trade_identity() {
        let trade = trade(Some("buy"), "59.50", "0.20");
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut stats = MakerStats::default();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let mut fills = Vec::new();

        collect_current_run_fills(
            vec![trade.clone()],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        collect_current_run_fills(
            vec![trade],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        assert_eq!(stats.fills(), 1);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, Some(42));
        assert_eq!(fills[0].order_id, Some(7));
        assert_eq!(fills[0].origin, "current_run_rest_trade");
        assert!((ledger.expected_position - 0.2).abs() < 1e-9);
    }

    fn order_update(fill_qty: &str, avg: &str) -> OrderUpdate {
        OrderUpdate {
            seq: 10,
            order_id: 7,
            cl_ord_id: Some("sxmk-0123456789ab-q00000001b0".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            qty: "0.20".to_string(),
            fill_qty: fill_qty.to_string(),
            fill_avg_price: avg.to_string(),
            price: "59.50".to_string(),
            status: standx_sdk::models::OrderStatus::PartiallyFilled,
            reduce_only: false,
            updated_at: "2026-07-10T00:00:01Z".to_string(),
        }
    }

    fn account_trade(side: OrderSide, qty: &str) -> TradeUpdate {
        TradeUpdate {
            seq: 11,
            trade_id: 42,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side,
            price: "59.50".to_string(),
            qty: qty.to_string(),
            trade_ts: "2026-07-10T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn websocket_then_rest_trade_is_not_double_counted() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        apply_order_update(
            &mut ledger,
            &order_update("0.20", "59.50"),
            "BTC-USD",
            "sxmk-0123456789ab-",
            &mut stats,
            &mut fills,
        )
        .unwrap();
        apply_account_trade(
            &mut ledger,
            account_trade(OrderSide::Buy, "0.20"),
            "BTC-USD",
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.20")],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 1);
        assert_eq!(fills.len(), 1);
        assert!((ledger.expected_position - 0.20).abs() < 1e-9);
    }

    #[test]
    fn rest_then_websocket_trade_is_not_double_counted() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.20")],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        apply_account_trade(
            &mut ledger,
            account_trade(OrderSide::Buy, "0.20"),
            "BTC-USD",
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 1);
        assert_eq!(fills.len(), 1);
        assert!((ledger.expected_position - 0.20).abs() < 1e-9);
    }

    #[test]
    fn historical_trade_without_current_run_order_is_ignored() {
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        let mut ledger = MakerLedger::new(-0.13);
        collect_current_run_fills(
            vec![trade(Some("sell"), "59.50", "0.20")],
            &mut ledger,
            1_783_000_000,
            1_784_000_000,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 0);
        assert!(fills.is_empty());
        assert_eq!(ledger.expected_position, -0.13);
    }

    #[test]
    fn current_run_trade_outside_session_is_rejected() {
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let error = collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.20")],
            &mut ledger,
            1_783_700_000,
            1_783_700_100,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside the session"));
    }

    #[test]
    fn current_run_client_order_ids_are_bounded_and_scoped() {
        let prefix = "sxmk-0123456789ab-";
        let quote = maker::quote_client_order_id(prefix, u64::MAX, OrderSide::Sell, u32::MAX);
        let exit = maker::exit_client_order_id(prefix, u64::MAX);
        assert!(quote.starts_with(prefix));
        assert!(exit.starts_with(prefix));
        assert!(quote.len() <= 41, "{quote}");
        assert!(exit.len() <= 41, "{exit}");
    }
}
