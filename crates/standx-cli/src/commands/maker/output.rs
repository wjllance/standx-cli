use super::*;
use standx_maker::{self as maker, Action, MakerConfig, MakerStats};
use standx_sdk::models::{Balance, OrderSide};

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
    // Real venue account snapshot. Present only in live mode.
    account: Option<&Balance>,
    actions: &[Action],
    // Paper-mode simulated fills this cycle: (side, price, qty). Empty in live.
    fills: &[(OrderSide, f64, f64)],
    stats: &MakerStats,
    // Some(vol_bps) when the volatility breaker halted quoting this cycle.
    halt_vol_bps: Option<f64>,
    cfg: &MakerConfig,
) {
    use maker::format_decimals;

    let pnl = stats.pnl(position, mark);

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
                    "account": account.map(account_json),
                    "holds": holds, "places": places, "cancels": cancels,
                    "fills": fills.len(),
                    "pnl": (pnl * 1e6).round() / 1e6,
                    "fills_total": stats.fills(),
                    "uptime_pct": (stats.uptime_pct() * 10.0).round() / 10.0,
                    "avg_capture_bps": (stats.avg_spread_capture_bps() * 100.0).round() / 100.0,
                    "halted": halt_vol_bps.is_some(),
                    "vol_bps": halt_vol_bps.map(|v| (v * 100.0).round() / 100.0),
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
            let mut fill_note = if fills.is_empty() {
                String::new()
            } else {
                format!(" fill={}", fills.len())
            };
            if let Some(v) = halt_vol_bps {
                fill_note.push_str(&format!(" ⚡HALT vol={:.1}bps", v));
            }
            println!(
                "[{}] #{} mark={} bid={} ask={} pos={} pnl={:.2} | hold={} place={} cancel={}{}",
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
                pnl,
                holds,
                places,
                cancels,
                fill_note
            );
            if let Some(account) = account {
                println!(
                    "    ACCOUNT balance={} equity={} available={} upnl={}",
                    format_account_amount(&account.balance),
                    format_account_amount(&account.equity),
                    format_account_amount(&account.cross_available),
                    format_account_amount(&account.upnl),
                );
            }
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

fn account_json(account: &Balance) -> serde_json::Value {
    serde_json::json!({
        "balance": account.balance,
        "equity": account.equity,
        "available": account.cross_available,
        "upnl": account.upnl,
    })
}

fn format_account_amount(value: &str) -> String {
    value
        .parse::<f64>()
        .ok()
        .filter(|amount| amount.is_finite())
        .map(|amount| format!("{amount:.2}"))
        .unwrap_or_else(|| value.to_string())
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
    use maker::format_decimals;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn balance() -> Balance {
        Balance {
            balance: "100.125".into(),
            cross_available: "80.5".into(),
            cross_balance: "100.125".into(),
            cross_margin: "19.625".into(),
            cross_upnl: "1.25".into(),
            equity: "101.375".into(),
            isolated_balance: "0".into(),
            isolated_upnl: "0".into(),
            locked: "0".into(),
            pnl_24h: "2.5".into(),
            pnl_freeze: "0".into(),
            upnl: "1.25".into(),
        }
    }

    #[test]
    fn account_snapshot_uses_real_balance_fields() {
        let json = account_json(&balance());
        assert_eq!(json["balance"], "100.125");
        assert_eq!(json["equity"], "101.375");
        assert_eq!(json["available"], "80.5");
        assert_eq!(json["upnl"], "1.25");
    }

    #[test]
    fn account_amounts_are_compact_without_hiding_invalid_values() {
        assert_eq!(format_account_amount("101.375"), "101.38");
        assert_eq!(format_account_amount("-0.005"), "-0.01");
        assert_eq!(format_account_amount("unavailable"), "unavailable");
    }
}
