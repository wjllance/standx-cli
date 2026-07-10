use super::output::{emit_maker_cycle, log_maker_event};
use super::{
    is_maker_order, is_order_rejection, open_qty_adopts, pending_covers_slot, PendingPlace,
    MAKER_CL_ORD_ID_PREFIX,
};
use crate::cli::*;
use anyhow::Result;
use standx_maker::{self as maker, MakerConfig, MakerStats, RestingQuote, VolBreaker};
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{OrderSide, OrderType, TimeInForce, Trade};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// One reconcile cycle over an already-acquired market snapshot.
/// Returns (places, cancels, holds, fills) counts. `sim_position` carries the
/// paper-mode simulated inventory across cycles (unused in live).
#[allow(clippy::too_many_arguments)]
pub(super) async fn maker_cycle(
    client: &StandXClient,
    symbol: &str,
    cfg: &MakerConfig,
    live: bool,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
    inventory_exit_pct: f64,
    inventory_exit_qty: f64,
    resting: &mut Vec<RestingQuote>,
    adopted: &mut HashMap<String, (u32, f64, u64)>,
    pending: &mut Vec<PendingPlace>,
    inventory_exit_pending: &mut bool,
    maker_order_ids: &mut HashSet<u64>,
    seen_fill_ids: &mut HashSet<u64>,
    session_started_at: i64,
    sim_position: &mut f64,
    stats: &mut MakerStats,
    breaker: &mut VolBreaker,
    output_format: OutputFormat,
    order_response_health: Option<&AtomicBool>,
) -> Result<(u64, u64, u64, u64)> {
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
            return Ok((0, 0, 0, 0));
        }
        Some(CycleSkip::MissingTouch) => {
            // Fail-safe: without a touch we cannot guarantee no-cross pricing.
            eprintln!("⚠️  empty order book on {}; skipping this cycle", symbol);
            return Ok((0, 0, 0, 0));
        }
        None => preflight.halted,
    };

    // 2. Rebuild resting + position from the exchange (live) or keep the
    //    simulated book (paper).
    let position: f64;
    let mut fills: Vec<(OrderSide, f64, f64)> = Vec::new(); // paper sim only
    let mut exit_fill_observed = false;
    if live {
        let now = chrono::Utc::now().timestamp();
        let (orders, positions, filled_orders, trades) = tokio::join!(
            client.get_open_orders(Some(symbol)),
            client.get_positions(Some(symbol)),
            client.get_order_history(Some(symbol), Some(100)),
            client.get_user_trades(symbol, session_started_at, now, Some(500)),
        );
        let orders = orders?;
        let positions = positions?;
        let filled_orders = filled_orders?;
        let trades = trades?;

        // Open maker orders identify partial fills; historical maker orders
        // identify a quote that fully filled between two polling cycles.
        let mut exit_order_ids = HashSet::new();
        for order in orders.iter().chain(filled_orders.iter()) {
            if is_maker_order(order) {
                let order_id = order.id.parse::<u64>().map_err(|_| {
                    anyhow::anyhow!(
                        "maker-owned order has non-integer exchange ID '{}'",
                        order.id
                    )
                })?;
                maker_order_ids.insert(order_id);
                if order
                    .cl_ord_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("sxmk-exit-"))
                {
                    exit_order_ids.insert(order_id);
                }
            }
        }

        for trade in trades {
            let Some(order_id) = trade.order_id else {
                continue;
            };
            if !maker_order_ids.contains(&order_id) {
                continue;
            }
            if trade.id == 0 {
                return Err(anyhow::anyhow!(
                    "maker fill for order {} has no stable trade ID",
                    order_id
                ));
            }
            if !seen_fill_ids.insert(trade.id) {
                continue;
            }
            let (side, price, qty) = maker_trade_fill(&trade)?;
            stats.record_fill(side, price, qty, mark);
            fills.push((side, price, qty));
            exit_fill_observed |= exit_order_ids.contains(&order_id);
        }

        position = positions
            .iter()
            .filter(|p| p.symbol.eq_ignore_ascii_case(symbol))
            .map(|p| {
                let qty: f64 = p.qty.parse().unwrap_or(0.0);
                match p.side {
                    Some(OrderSide::Sell) => -qty,
                    _ => qty,
                }
            })
            .sum();

        let tick = cfg.price_tick();
        *resting = orders
            .into_iter()
            .filter(is_maker_order)
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
                                        && open_qty_adopts(qty, p.qty)
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
                fills.push((q.side, q.price, q.qty));
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
            Action::Place(q) if live && pending_covers_slot(pending, q.side, q.level) => {
                log_maker_event(
                    output_format,
                    symbol,
                    cycle,
                    "place_pending",
                    q.side,
                    q.level,
                    q.price,
                    cfg.price_decimals,
                    "awaiting asynchronous order confirmation",
                );
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
                                log_maker_event(
                                    output_format,
                                    symbol,
                                    cycle,
                                    "cancel_noop",
                                    *side,
                                    *level,
                                    *price,
                                    cfg.price_decimals,
                                    "order already gone",
                                );
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
                    if !order_response_health.is_some_and(|health| health.load(Ordering::Acquire)) {
                        return Err(anyhow::anyhow!(
                            "order-response stream is unhealthy; refusing live placement"
                        ));
                    }
                    let cl_ord_id = format!("{}{}", MAKER_CL_ORD_ID_PREFIX, uuid::Uuid::new_v4());
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
                            log_maker_event(
                                output_format,
                                symbol,
                                cycle,
                                "place_rejected",
                                q.side,
                                q.level,
                                q.price,
                                cfg.price_decimals,
                                "post-only rejected",
                            );
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
            if !order_response_health.is_some_and(|health| health.load(Ordering::Acquire)) {
                return Err(anyhow::anyhow!(
                    "order-response stream is unhealthy; refusing inventory exit"
                ));
            }
            let cl_ord_id = format!("{}exit-{}", MAKER_CL_ORD_ID_PREFIX, uuid::Uuid::new_v4());
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
            log_maker_event(
                output_format,
                symbol,
                cycle,
                "inventory_exit_submitted",
                exit.side,
                0,
                mark,
                cfg.price_decimals,
                "reduce-only market order submitted after maker book cleared",
            );
        }
    }

    // 5. Telemetry uses exact ledger fills in live mode and simulated fills
    // in paper mode; never infer a fill from a position delta.
    let two_sided = resting.iter().any(|r| r.side == OrderSide::Buy)
        && resting.iter().any(|r| r.side == OrderSide::Sell);
    stats.end_cycle(position, two_sided);

    // 6. Emit.
    emit_maker_cycle(
        output_format,
        live,
        symbol,
        cycle,
        mark,
        best_bid,
        best_ask,
        position,
        &actions,
        &fills,
        stats,
        halted.then(|| breaker.vol_bps()),
        cfg,
    );

    Ok((places, cancels, holds, fills.len() as u64))
}

/// Decode a venue fill strictly enough for accounting. A maker fill with
/// missing side, price, or quantity is not silently guessed from position.
fn maker_trade_fill(trade: &Trade) -> Result<(OrderSide, f64, f64)> {
    let side = match trade.side.as_deref() {
        Some(side) if side.eq_ignore_ascii_case("buy") => OrderSide::Buy,
        Some(side) if side.eq_ignore_ascii_case("sell") => OrderSide::Sell,
        _ => {
            return Err(anyhow::anyhow!(
                "maker trade {} is missing a valid side",
                trade.id
            ));
        }
    };
    let price = trade.price.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "maker trade {} has invalid price '{}'",
            trade.id,
            trade.price
        )
    })?;
    let qty = trade
        .qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("maker trade {} has invalid qty '{}'", trade.id, trade.qty))?;
    if !price.is_finite() || price <= 0.0 || !qty.is_finite() || qty <= 0.0 {
        return Err(anyhow::anyhow!(
            "maker trade {} has non-positive price/qty",
            trade.id
        ));
    }
    Ok((side, price, qty))
}

/// Cancel maker-owned orders with retries, preserving manual/API orders.
pub(super) async fn cancel_maker_orders_with_retry(
    client: &StandXClient,
    symbol: &str,
    attempts: u32,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        let result = async {
            let orders = client.get_open_orders(Some(symbol)).await?;
            let order_ids = orders
                .iter()
                .filter(|order| is_maker_order(order))
                .map(|order| {
                    order.id.parse::<i64>().map_err(|_| {
                        anyhow::anyhow!(
                            "maker-owned order has non-integer exchange ID '{}'",
                            order.id
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            client.cancel_orders(&order_ids).await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                eprintln!(
                    "⚠️  maker-order cancellation attempt {}/{} failed: {}",
                    attempt, attempts, e
                );
                last_err = Some(e);
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    // Verify only maker-owned orders. Foreign orders are intentionally left
    // untouched and must not turn a clean maker shutdown into an error.
    match client.get_open_orders(Some(symbol)).await {
        Ok(orders) if orders.iter().all(|order| !is_maker_order(order)) => {
            println!("✅ All maker-owned {} orders cancelled", symbol);
            Ok(())
        }
        Ok(orders) => {
            let ids: Vec<_> = orders
                .iter()
                .filter(|order| is_maker_order(order))
                .map(|order| order.id.as_str())
                .collect();
            Err(anyhow::anyhow!(
                "⚠️  RESIDUAL MAKER ORDERS on {} after cancellation: [{}] — inspect or cancel manually with 'standx order cancel-all {}'",
                symbol,
                ids.join(", "),
                symbol
            ))
        }
        Err(e) => match last_err {
            Some(cancel_err) => Err(anyhow::anyhow!(
                "maker-order cancellation failed ({}) and verification failed ({}) — check open orders manually",
                cancel_err,
                e
            )),
            None => Err(anyhow::anyhow!(
                "maker-order cancellation succeeded but verification failed ({}) — check open orders manually",
                e
            )),
        },
    }
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
}
