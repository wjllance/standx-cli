#[cfg(test)]
use super::ledger::apply_order_update;
#[cfg(test)]
use super::ledger::maker_trade_fill;
use super::ledger::{adopt_order, apply_rest_trade};
use super::model::{is_current_run_order, is_order_rejection, position_for_symbol, PendingPlace};
use super::output::{emit_maker_cycle, log_maker_event, CycleOutput, MakerLogEvent};
use super::pipeline::{fetch_snapshot, CycleRequest, CycleResult, CycleState};
use super::recovery::{
    recover_current_run_order_ids_for_reconciliation, PositionGap, PositionReconciliationError,
};
use crate::cli::*;
use anyhow::Result;
use standx_maker::{self as maker, MakerFill, MakerLedger, MakerStats, RestingQuote};
#[cfg(test)]
use standx_sdk::account_stream::OrderUpdate;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::models::{Balance, OrderSide, OrderType, TimeInForce, Trade};
use standx_sdk::order_response::OrderResponseHealth;
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
        max_divergence_bps,
        inventory_exit_pct,
        inventory_exit_qty,
        session_started_at,
        run_order_prefix,
        starting_position,
        output_format,
        order_response_health,
    } = request;
    let CycleState {
        resting,
        adopted,
        pending,
        inventory_exit_pending,
        ledger,
        sim_position,
        stats,
        breaker,
    } = state;
    use maker::{
        format_decimals, paper_quote_filled, Action, CycleInput, CycleSkip, MarketSnapshot,
    };

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
        Some(CycleSkip::MarkMidDivergence { divergence_bps }) => {
            let live_str = if live { "live" } else { "paper" };
            match output_format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "cycle": cycle, "mode": live_str, "symbol": symbol,
                            "action": "skip", "reason": "mark_mid_divergence",
                            "mark": format_decimals(mark, cfg.price_decimals),
                            "divergence_bps": (divergence_bps * 100.0).round() / 100.0,
                            "max_divergence_bps": max_divergence_bps,
                        })
                    );
                }
                _ => {
                    eprintln!(
                        "⚠️  #{} mark/mid divergence {:.1}bps > {}bps — skipping cycle (no actions)",
                        cycle, divergence_bps, max_divergence_bps
                    );
                }
            }
            return Ok(CycleResult::default());
        }
        Some(CycleSkip::MissingTouch) => {
            // Fail-safe: without a touch we cannot guarantee no-cross pricing.
            eprintln!("⚠️  empty order book on {}; skipping this cycle", symbol);
            return Ok(CycleResult::default());
        }
        None => preflight.halted,
    };

    // 2. Rebuild resting + position from the exchange (live) or keep the
    //    simulated book (paper).
    let position: f64;
    let mut account_balance: Option<Balance> = None;
    let mut fills: Vec<MakerFill> = Vec::new();
    let mut exit_fill_observed = false;
    if live {
        let now = chrono::Utc::now().timestamp();
        let snapshot = fetch_snapshot(client, symbol, session_started_at, now).await?;
        let mut orders = snapshot.open_orders;
        let filled_orders = snapshot.filled_orders;
        let trades = snapshot.trades;
        account_balance = Some(snapshot.balance);

        // Open maker orders identify partial fills; historical maker orders
        // identify a quote that fully filled between two polling cycles.
        for order in orders.iter().chain(filled_orders.iter()) {
            adopt_order(ledger, order, run_order_prefix)?;
        }

        exit_fill_observed |= collect_current_run_fills(
            trades,
            ledger,
            session_started_at,
            now,
            mark,
            stats,
            &mut fills,
        )?;

        let mut observed_position = position_for_symbol(&snapshot.positions, symbol)?;
        let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
        if (observed_position - ledger.expected_position).abs() > qty_tolerance {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let retry_now = chrono::Utc::now().timestamp();
            let (retry_orders, retry_filled_orders, retry_trades) = tokio::join!(
                client.get_open_orders(Some(symbol)),
                client.get_order_history(Some(symbol), Some(100)),
                client.get_user_trades(symbol, session_started_at, retry_now, Some(500)),
            );
            orders = retry_orders?;
            let retry_filled_orders = retry_filled_orders?;
            for order in orders.iter().chain(retry_filled_orders.iter()) {
                adopt_order(ledger, order, run_order_prefix)?;
            }
            let retry_trades = retry_trades?;
            recover_current_run_order_ids_for_reconciliation(
                client,
                &retry_trades,
                PositionGap {
                    expected: ledger.expected_position,
                    observed: observed_position,
                    qty_tolerance,
                    run_order_prefix,
                },
                ledger,
            )
            .await;
            exit_fill_observed |= collect_current_run_fills(
                retry_trades,
                ledger,
                session_started_at,
                retry_now,
                mark,
                stats,
                &mut fills,
            )?;
            let retry_positions = client.get_positions(Some(symbol)).await?;
            observed_position = position_for_symbol(&retry_positions, symbol)?;
            if (observed_position - ledger.expected_position).abs() > qty_tolerance {
                if output_format == OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "symbol": symbol,
                            "cycle": cycle,
                            "action": "position_reconciliation",
                            "event": "failed",
                            "expected_position": ledger.expected_position,
                            "observed_position": observed_position,
                            "message": "venue position cannot be explained by current-run maker fills",
                        })
                    );
                } else {
                    eprintln!(
                        "⚠️  position reconciliation failed: expected {:+.8}, observed {:+.8}",
                        ledger.expected_position, observed_position
                    );
                }
                return Err(anyhow::Error::new(PositionReconciliationError {
                    expected: ledger.expected_position,
                    observed: observed_position,
                }));
            }
        }
        position = observed_position;

        let tick = cfg.price_tick();
        *resting = orders
            .into_iter()
            .filter(|order| is_current_run_order(order, run_order_prefix))
            .map(|o| {
                let price: f64 = o.price.parse().unwrap_or(0.0);
                let qty: f64 = o.qty.parse().unwrap_or(0.0);
                let (level, ref_center, placed_at_cycle) = match adopted.get(&o.id) {
                    Some(&meta) => meta,
                    None => {
                        // Try to adopt from a recent place by side + price,
                        // tolerating a shrunk qty from a partial fill (see
                        // open_qty_adopts).
                        let matched = o
                            .cl_ord_id
                            .as_ref()
                            .and_then(|cl_ord_id| {
                                pending.iter().position(|p| p.cl_ord_id == *cl_ord_id)
                            })
                            .or_else(|| {
                                // Backward-compatible fallback for orders
                                // created before client IDs were enabled.
                                pending.iter().position(|p| {
                                    p.side == o.side
                                        && (p.price - price).abs() < tick / 2.0
                                        && maker::open_qty_adopts(qty, p.qty)
                                })
                            });
                        let meta = match matched {
                            Some(idx) => {
                                let p = pending.remove(idx);
                                (p.level, p.ref_center, p.cycle)
                            }
                            // An older maker order without in-memory state:
                            // sentinel level makes reconciliation replace it.
                            // Manual orders were filtered above and cannot
                            // enter the strategy state.
                            None => (u32::MAX, mark, cycle),
                        };
                        adopted.insert(o.id.clone(), meta);
                        meta
                    }
                };
                RestingQuote {
                    order_id: Some(o.id),
                    side: o.side,
                    level,
                    price,
                    qty,
                    ref_center,
                    placed_at_cycle,
                }
            })
            .collect();
        // Places older than 2 cycles never showed up as open orders —
        // likely rejected (e.g. ALO would-cross) or fully filled on arrival.
        pending.retain(|p| cycle.saturating_sub(p.cycle) <= 2);
        adopted.retain(|id, _| resting.iter().any(|r| r.order_id.as_deref() == Some(id)));
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
    let pending_slots = pending
        .iter()
        .map(|place| (place.side, place.level))
        .collect::<Vec<_>>();
    let plan = maker::plan_cycle(
        cfg,
        CycleInput {
            cycle,
            market,
            position,
            resting,
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
                        pending.iter().map(|place| maker::QuoteSlot {
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

    // 4. Execute. Business rejections (post-only would-cross, order already
    //    gone) are expected and logged inline; only transient failures
    //    propagate as cycle errors toward the fail-safe.
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
                    if let Some(id) = order_id {
                        match client.cancel_order(symbol, id).await {
                            Ok(()) => {
                                adopted.remove(id);
                                cancels += 1;
                            }
                            Err(e) if is_order_rejection(&e) => {
                                // Order already gone (filled or cancelled
                                // out from under us) — that IS the goal.
                                adopted.remove(id);
                                cancels += 1;
                                log_maker_event(MakerLogEvent {
                                    output_format,
                                    symbol,
                                    cycle,
                                    action: "cancel_noop",
                                    side: *side,
                                    level: *level,
                                    price: *price,
                                    price_decimals: cfg.price_decimals,
                                    detail: "order already gone",
                                });
                            }
                            // Transient (network / 5xx) → fail-safe path.
                            Err(e) => return Err(e.into()),
                        }
                    }
                } else {
                    resting.retain(|r| !(r.side == *side && r.level == *level));
                    cancels += 1;
                }
            }
            Action::Place(q) => {
                if live {
                    if let Some(reason) = unhealthy_order_response(order_response_health) {
                        return Err(anyhow::anyhow!("{reason}; refusing live placement"));
                    }
                    let cl_ord_id =
                        maker::quote_client_order_id(run_order_prefix, cycle, q.side, q.level);
                    match client
                        .create_order(CreateOrderParams {
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
                        })
                        .await
                    {
                        Ok(submission) => {
                            pending.push(PendingPlace {
                                request_id: submission.id,
                                cl_ord_id,
                                side: q.side,
                                price: q.price,
                                qty: q.qty,
                                level: q.level,
                                ref_center,
                                cycle,
                            });
                            places += 1;
                        }
                        Err(e) if is_order_rejection(&e) => {
                            // Post-only would-cross etc. — expected in fast
                            // markets. Re-quote next cycle, don't fail-safe.
                            log_maker_event(MakerLogEvent {
                                output_format,
                                symbol,
                                cycle,
                                action: "place_rejected",
                                side: q.side,
                                level: q.level,
                                price: q.price,
                                price_decimals: cfg.price_decimals,
                                detail: "post-only rejected",
                            });
                        }
                        Err(e) => return Err(e.into()),
                    }
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
        if resting.is_empty() && pending.is_empty() {
            if let Some(reason) = unhealthy_order_response(order_response_health) {
                return Err(anyhow::anyhow!("{reason}; refusing inventory exit"));
            }
            let cl_ord_id = maker::exit_client_order_id(run_order_prefix, cycle);
            client
                .create_order(CreateOrderParams {
                    symbol: symbol.to_string(),
                    cl_ord_id: Some(cl_ord_id),
                    side: exit.side,
                    order_type: OrderType::Market,
                    quantity: format_decimals(exit.qty, cfg.qty_decimals),
                    price: None,
                    time_in_force: None,
                    reduce_only: true,
                    stop_price: None,
                    sl_price: None,
                    tp_price: None,
                })
                .await?;
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
    let two_sided = resting.iter().any(|r| r.side == OrderSide::Buy)
        && resting.iter().any(|r| r.side == OrderSide::Sell);
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
    fn rest_then_websocket_only_accounts_cumulative_delta() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.10")],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        apply_order_update(
            &mut ledger,
            &order_update("0.20", "59.50"),
            "BTC-USD",
            "sxmk-0123456789ab-",
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 2);
        assert_eq!(fills.len(), 2);
        assert!((fills[1].qty - 0.10).abs() < 1e-9);
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
