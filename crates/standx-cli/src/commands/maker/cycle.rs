use super::output::{emit_maker_cycle, log_maker_event};
use super::{is_order_rejection, open_qty_adopts, PendingPlace};
use crate::cli::*;
use anyhow::Result;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{OrderSide, OrderType, TimeInForce};
use std::collections::HashMap;
use std::time::Duration;

/// One reconcile cycle over an already-acquired market snapshot.
/// Returns (places, cancels, holds, fills) counts. `sim_position` carries the
/// paper-mode simulated inventory across cycles (unused in live).
#[allow(clippy::too_many_arguments)]
pub(super) async fn maker_cycle(
    client: &StandXClient,
    symbol: &str,
    cfg: &standx_sdk::maker::MakerConfig,
    live: bool,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
    resting: &mut Vec<standx_sdk::maker::RestingQuote>,
    adopted: &mut HashMap<String, (u32, f64, u64)>,
    pending: &mut Vec<PendingPlace>,
    sim_position: &mut f64,
    output_format: OutputFormat,
) -> Result<(u64, u64, u64, u64)> {
    use standx_sdk::maker::{
        compute_desired_quotes, format_decimals, mark_mid_divergence_bps, paper_quote_filled,
        reconcile, skew_center, Action, RestingQuote,
    };

    // 1. Sanity guard: when mark and the book mid disagree, at least one
    //    data source is wrong (stale feed, bad print, dislocated book).
    //    Acting on it is unsafe in every direction, so do nothing this
    //    cycle: resting quotes stay untouched. Not a fail-safe error.
    if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
        let divergence = mark_mid_divergence_bps(mark, bid, ask);
        if divergence > max_divergence_bps {
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
                            "divergence_bps": (divergence * 100.0).round() / 100.0,
                            "max_divergence_bps": max_divergence_bps,
                        })
                    );
                }
                _ => {
                    eprintln!(
                        "⚠️  #{} mark/mid divergence {:.1}bps > {}bps — skipping cycle (no actions)",
                        cycle, divergence, max_divergence_bps
                    );
                }
            }
            return Ok((0, 0, 0, 0));
        }
    }

    if live && (best_bid.is_none() || best_ask.is_none()) {
        // Fail-safe: without a touch we cannot guarantee no-cross pricing.
        eprintln!("⚠️  empty order book on {}; skipping this cycle", symbol);
        return Ok((0, 0, 0, 0));
    }

    // 2. Rebuild resting + position from the exchange (live) or keep the
    //    simulated book (paper).
    let position: f64;
    let mut fills: Vec<(OrderSide, f64, f64)> = Vec::new(); // paper sim only
    if live {
        let (orders, positions) = tokio::join!(
            client.get_open_orders(Some(symbol)),
            client.get_positions(Some(symbol))
        );
        let orders = orders?;
        let positions = positions?;

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
            .map(|o| {
                let price: f64 = o.price.parse().unwrap_or(0.0);
                let qty: f64 = o.qty.parse().unwrap_or(0.0);
                let (level, ref_center, placed_at_cycle) = match adopted.get(&o.id) {
                    Some(&meta) => meta,
                    None => {
                        // Try to adopt from a recent place by side + price,
                        // tolerating a shrunk qty from a partial fill (see
                        // open_qty_adopts).
                        let matched = pending.iter().position(|p| {
                            p.side == o.side
                                && (p.price - price).abs() < tick / 2.0
                                && open_qty_adopts(qty, p.qty)
                        });
                        let meta = match matched {
                            Some(idx) => {
                                let p = pending.remove(idx);
                                (p.level, p.ref_center, p.cycle)
                            }
                            // Unknown order (manual or unmatched): sentinel
                            // level so reconcile cancels it as stale — the
                            // bot owns all orders on this symbol.
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
                fills.push((q.side, q.price, q.qty));
            } else {
                i += 1;
            }
        }
        position = *sim_position;
    }

    // 3. Decide.
    let desired = compute_desired_quotes(cfg, mark, best_bid, best_ask, position);
    let actions = reconcile(
        cfg, mark, position, best_bid, best_ask, &desired, resting, cycle,
    );

    // The quote center these places are built around — the anti-flicker
    // anchor stored on each placed quote (equals mark when skew is off).
    let ref_center = skew_center(cfg, mark, position);

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
                    match client
                        .create_order(CreateOrderParams {
                            symbol: symbol.to_string(),
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
                        Ok(_) => {
                            pending.push(PendingPlace {
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

    // 5. Emit.
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
        cfg,
    );

    Ok((places, cancels, holds, fills.len() as u64))
}

/// Cancel-all with retries; verifies the book is actually clean afterwards.
pub(super) async fn cancel_all_with_retry(
    client: &StandXClient,
    symbol: &str,
    attempts: u32,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        match client.cancel_all_orders(symbol).await {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                eprintln!(
                    "⚠️  cancel-all attempt {}/{} failed: {}",
                    attempt, attempts, e
                );
                last_err = Some(e.into());
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    // Verify: a failed cancel leaves live orders unattended.
    match client.get_open_orders(Some(symbol)).await {
        Ok(orders) if orders.is_empty() => {
            println!("✅ All {} orders cancelled", symbol);
            Ok(())
        }
        Ok(orders) => {
            let ids: Vec<_> = orders.iter().map(|o| o.id.as_str()).collect();
            Err(anyhow::anyhow!(
                "⚠️  RESIDUAL ORDERS on {} after cancel-all: [{}] — cancel manually with 'standx order cancel-all {}'",
                symbol,
                ids.join(", "),
                symbol
            ))
        }
        Err(e) => match last_err {
            Some(cancel_err) => Err(anyhow::anyhow!(
                "cancel-all failed ({}) and verification failed ({}) — check open orders manually",
                cancel_err,
                e
            )),
            None => Err(anyhow::anyhow!(
                "cancel-all succeeded but verification failed ({}) — check open orders manually",
                e
            )),
        },
    }
}
