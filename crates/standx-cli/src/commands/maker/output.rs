use super::*;
use standx_sdk::models::OrderSide;

/// Per-cycle output: one human line + indented actions, or JSON lines.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_maker_cycle(
    output_format: OutputFormat,
    live: bool,
    symbol: &str,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    position: f64,
    actions: &[standx_sdk::maker::Action],
    // Paper-mode simulated fills this cycle: (side, price, qty). Empty in live.
    fills: &[(OrderSide, f64, f64)],
    cfg: &standx_sdk::maker::MakerConfig,
) {
    use standx_sdk::maker::{format_decimals, Action};

    let mode = if live { "live" } else { "paper" };
    let counts = actions.iter().fold((0, 0, 0), |mut acc, a| {
        match a {
            Action::Place(_) => acc.1 += 1,
            Action::Cancel { .. } => acc.2 += 1,
            Action::Hold { .. } => acc.0 += 1,
        }
        acc
    });
    let (holds, places, cancels) = counts;

    match output_format {
        OutputFormat::Json => {
            let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            for (side, price, qty) in fills {
                println!(
                    "{}",
                    serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "action": "fill", "side": side,
                        "price": format_decimals(*price, cfg.price_decimals),
                        "qty": format_decimals(*qty, cfg.qty_decimals),
                    })
                );
            }
            for a in actions {
                let obj = match a {
                    Action::Place(q) => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "place", "side": q.side, "level": q.level,
                        "price": format_decimals(q.price, cfg.price_decimals),
                        "qty": format_decimals(q.qty, cfg.qty_decimals),
                    }),
                    Action::Cancel {
                        order_id,
                        side,
                        level,
                        price,
                        reason,
                    } => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "cancel", "side": side, "level": level,
                        "price": format_decimals(*price, cfg.price_decimals),
                        "reason": reason.as_str(), "order_id": order_id,
                    }),
                    Action::Hold {
                        side,
                        level,
                        price,
                        age_cycles,
                        drift_bps,
                    } => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "hold", "side": side, "level": level,
                        "price": format_decimals(*price, cfg.price_decimals),
                        "age_cycles": age_cycles,
                        "drift_bps": (drift_bps * 100.0).round() / 100.0,
                    }),
                };
                println!("{}", obj);
            }
            println!(
                "{}",
                serde_json::json!({
                    "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                    "action": "cycle_summary",
                    "mark": format_decimals(mark, cfg.price_decimals),
                    "best_bid": best_bid, "best_ask": best_ask,
                    "position": position,
                    "holds": holds, "places": places, "cancels": cancels,
                    "fills": fills.len(),
                })
            );
        }
        OutputFormat::Quiet => {
            for (side, price, qty) in fills {
                println!(
                    "fill {} @ {} x {}",
                    side_str(*side),
                    format_decimals(*price, cfg.price_decimals),
                    format_decimals(*qty, cfg.qty_decimals)
                );
            }
            // Only mutations and their reasons.
            for a in actions {
                match a {
                    Action::Place(q) => println!(
                        "place {} L{} @ {}",
                        side_str(q.side),
                        q.level,
                        format_decimals(q.price, cfg.price_decimals)
                    ),
                    Action::Cancel {
                        side,
                        level,
                        price,
                        reason,
                        ..
                    } => println!(
                        "cancel {} L{} @ {} ({})",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        reason.as_str()
                    ),
                    Action::Hold { .. } => {}
                }
            }
        }
        _ => {
            let now = chrono::Local::now().format("%H:%M:%S");
            let fill_note = if fills.is_empty() {
                String::new()
            } else {
                format!(" fill={}", fills.len())
            };
            println!(
                "[{}] #{} mark={} bid={} ask={} pos={} | hold={} place={} cancel={}{}",
                now,
                cycle,
                format_decimals(mark, cfg.price_decimals),
                best_bid
                    .map(|b| format_decimals(b, cfg.price_decimals))
                    .unwrap_or_else(|| "-".into()),
                best_ask
                    .map(|a| format_decimals(a, cfg.price_decimals))
                    .unwrap_or_else(|| "-".into()),
                format_decimals(position, cfg.qty_decimals),
                holds,
                places,
                cancels,
                fill_note
            );
            for (side, price, qty) in fills {
                println!(
                    "    FILL   {} @ {} x {}",
                    side_str(*side),
                    format_decimals(*price, cfg.price_decimals),
                    format_decimals(*qty, cfg.qty_decimals)
                );
            }
            for a in actions {
                match a {
                    Action::Place(q) => println!(
                        "    PLACE  {} L{} @ {} x {}",
                        side_str(q.side),
                        q.level,
                        format_decimals(q.price, cfg.price_decimals),
                        format_decimals(q.qty, cfg.qty_decimals)
                    ),
                    Action::Cancel {
                        side,
                        level,
                        price,
                        reason,
                        ..
                    } => println!(
                        "    CANCEL {} L{} @ {} ({})",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        reason.as_str()
                    ),
                    Action::Hold {
                        side,
                        level,
                        price,
                        age_cycles,
                        drift_bps,
                    } => println!(
                        "    HOLD   {} L{} @ {} (age {} cycles, drift {:.1}bps)",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        age_cycles,
                        drift_bps
                    ),
                }
            }
        }
    }
}

fn side_str(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "buy ",
        OrderSide::Sell => "sell",
    }
}

/// Emit a one-off maker event (order rejection, no-op cancel) inline,
/// respecting the output format. Only reached in live mode.
#[allow(clippy::too_many_arguments)]
pub(super) fn log_maker_event(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    action: &str,
    side: OrderSide,
    level: u32,
    price: f64,
    price_decimals: u32,
    detail: &str,
) {
    use standx_sdk::maker::format_decimals;
    match output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    "cycle": cycle, "mode": "live", "symbol": symbol,
                    "action": action, "side": side, "level": level,
                    "price": format_decimals(price, price_decimals),
                    "detail": detail,
                })
            );
        }
        _ => {
            eprintln!(
                "    {} {} L{} @ {} — {}",
                action,
                side_str(side),
                level,
                format_decimals(price, price_decimals),
                detail
            );
        }
    }
}
