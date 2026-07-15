use super::*;
use standx_maker::{self as maker, Action, MakerConfig, MakerStats};
use standx_sdk::models::{Balance, OrderSide};

/// Per-cycle output: one human line + indented actions, or JSON lines.
pub(super) struct CycleOutput<'a> {
    pub(super) output_format: OutputFormat,
    pub(super) live: bool,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
    pub(super) mark: f64,
    pub(super) best_bid: Option<f64>,
    pub(super) best_ask: Option<f64>,
    pub(super) market_source: &'static str,
    pub(super) market_fallback_reason: Option<&'static str>,
    pub(super) position: f64,
    pub(super) starting_position: f64,
    pub(super) account: Option<&'a Balance>,
    pub(super) actions: &'a [Action],
    pub(super) fills: &'a [MakerFill],
    pub(super) stats: &'a MakerStats,
    pub(super) halt_vol_bps: Option<f64>,
    pub(super) cfg: &'a MakerConfig,
}

pub(super) fn emit_maker_cycle(output: CycleOutput<'_>) {
    let CycleOutput {
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
        account,
        actions,
        fills,
        stats,
        halt_vol_bps,
        cfg,
    } = output;
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
            for fill in fills {
                println!(
                    "{}",
                    serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "action": "fill", "side": fill.side,
                        "price": format_decimals(fill.price, cfg.price_decimals),
                        "qty": format_decimals(fill.qty, cfg.qty_decimals),
                        "trade_id": fill.trade_id,
                        "order_id": fill.order_id,
                        "trade_ts": fill.trade_ts,
                        "origin": fill.origin,
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
                    "market_source": market_source,
                    "market_fallback_reason": market_fallback_reason,
                    "position": position,
                    "starting_position": starting_position,
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
            for fill in fills {
                println!(
                    "fill {} @ {} x {}",
                    side_str(fill.side),
                    format_decimals(fill.price, cfg.price_decimals),
                    format_decimals(fill.qty, cfg.qty_decimals)
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
            for fill in fills {
                println!(
                    "    FILL   {} @ {} x {}",
                    side_str(fill.side),
                    format_decimals(fill.price, cfg.price_decimals),
                    format_decimals(fill.qty, cfg.qty_decimals)
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
pub(super) struct MakerLogEvent<'a> {
    pub(super) output_format: OutputFormat,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
    pub(super) action: &'a str,
    pub(super) side: OrderSide,
    pub(super) level: u32,
    pub(super) price: f64,
    pub(super) price_decimals: u32,
    pub(super) detail: &'a str,
}

pub(super) fn log_maker_event(event: MakerLogEvent<'_>) {
    let MakerLogEvent {
        output_format,
        symbol,
        cycle,
        action,
        side,
        level,
        price,
        price_decimals,
        detail,
    } = event;
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

pub(super) fn emit_live_fill(
    fill: &MakerFill,
    symbol: &str,
    cycle: u64,
    output_format: OutputFormat,
) {
    match output_format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "fill",
                "origin": fill.origin,
                "order_id": fill.order_id,
                "trade_id": fill.trade_id,
                "trade_ts": fill.trade_ts,
                "side": fill.side,
                "price": fill.price,
                "qty": fill.qty,
            })
        ),
        _ => eprintln!(
            "⚡ account fill {:?} {} @ {} (order {})",
            fill.side,
            fill.qty,
            fill.price,
            fill.order_id.unwrap_or_default()
        ),
    }
}

pub(super) fn emit_reconciliation_state(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    event: &str,
    cause: &str,
    expected: f64,
    observed: f64,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "position_reconciliation",
                "event": event,
                "cause": cause,
                "expected_position": expected,
                "observed_position": observed,
            })
        );
    } else {
        eprintln!(
            "⚠️  position reconciliation {event} ({cause}): expected {expected:+.8}, observed {observed:+.8}"
        );
    }
}

pub(super) fn emit_stop_loss_triggered(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    pnl: f64,
    stop_loss: f64,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "stop_loss",
                "event": "triggered",
                "pnl": pnl,
                "stop_loss": stop_loss,
            })
        );
    } else {
        eprintln!(
            "🛑 stop-loss triggered: session PnL {pnl:+.2} breached -{stop_loss:.2}; shutting down"
        );
    }
}

pub(super) fn emit_reconciliation_snapshot_error(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    message: &str,
) {
    // Precursor signal: a failed reconciliation snapshot inside the freeze
    // window is an early warning that the fail-safe may not converge. Surface
    // it on stdout (JSON mode) so ingest uploads it rather than losing it to
    // local stderr only.
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "position_reconciliation",
                "event": "snapshot_failed",
                "severity": "warning",
                "message": message,
            })
        );
    } else {
        eprintln!("⚠️  bounded position reconciliation snapshot failed: {message}");
    }
}

pub(super) fn emit_ledger_sync(
    output_format: OutputFormat,
    symbol: &str,
    starting_position: f64,
    baseline_mark: f64,
    historical_orders: usize,
    historical_trades: usize,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "action": "ledger_sync",
                "event": "complete",
                "starting_position": starting_position,
                "baseline_mark": baseline_mark,
                "pnl_baseline": 0.0,
                "historical_maker_orders": historical_orders,
                "historical_maker_trades_ignored": historical_trades,
                "history_window_seconds": LEDGER_HISTORY_WINDOW_SECS,
                "history_order_limit": ORDER_HISTORY_LIMIT,
                "history_trade_limit": TRADE_LOOKBACK_LIMIT,
                "current_run_fills": 0,
            })
        );
        if starting_position.abs() > f64::EPSILON {
            println!(
                "{}",
                serde_json::json!({
                    "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    "symbol": symbol,
                    "action": "inventory_adopted",
                    "event": "complete",
                    "starting_position": starting_position,
                    "baseline_mark": baseline_mark,
                    "pnl_baseline": 0.0,
                })
            );
        }
    } else {
        eprintln!(
            "✅ maker ledger synchronized: position={starting_position:+.8}, baseline mark={baseline_mark:.8}, ignored historical fills={historical_trades}"
        );
    }
}

pub(super) fn emit_startup_rejected(
    output_format: OutputFormat,
    symbol: &str,
    position: f64,
    max_position: f64,
) {
    let message = format!(
        "starting position {position:+.8} exceeds max_position {max_position:.8}; refusing live maker"
    );
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "action": "startup_rejected",
                "event": "position_over_limit",
                "position": position,
                "max_position": max_position,
                "message": message,
            })
        );
    } else {
        eprintln!("⚠️  {message}");
    }
}

/// The current instant as an RFC3339 string, truncated to whole seconds — the
/// timestamp format every maker telemetry line uses.
pub(super) fn ts_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Emit a skipped-cycle event. Unlike the previous inline handling, all three
/// reasons — including `MissingTouch` — now produce a JSON event, so an ingest
/// pipeline sees every skip rather than silently missing empty-book cycles.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_cycle_skip(
    output_format: OutputFormat,
    cycle: u64,
    symbol: &str,
    live: bool,
    mark: f64,
    price_decimals: u32,
    max_divergence_bps: f64,
    skip: maker::CycleSkip,
) {
    if output_format == OutputFormat::Json {
        let mut event = serde_json::json!({
            "ts": ts_now(),
            "cycle": cycle,
            "mode": if live { "live" } else { "paper" },
            "symbol": symbol,
            "action": "skip",
            "mark": maker::format_decimals(mark, price_decimals),
        });
        let fields = event.as_object_mut().expect("json object");
        match skip {
            maker::CycleSkip::CrossedBook => {
                fields.insert("reason".into(), "crossed_book".into());
            }
            maker::CycleSkip::MarkMidDivergence { divergence_bps } => {
                fields.insert("reason".into(), "mark_mid_divergence".into());
                fields.insert(
                    "divergence_bps".into(),
                    ((divergence_bps * 100.0).round() / 100.0).into(),
                );
                fields.insert("max_divergence_bps".into(), max_divergence_bps.into());
            }
            maker::CycleSkip::MissingTouch => {
                fields.insert("reason".into(), "missing_touch".into());
            }
        }
        println!("{event}");
        return;
    }
    match skip {
        maker::CycleSkip::CrossedBook => eprintln!(
            "⚠️  #{cycle} crossed order book on {symbol}; skipping cycle (no actions)"
        ),
        maker::CycleSkip::MarkMidDivergence { divergence_bps } => eprintln!(
            "⚠️  #{cycle} mark/mid divergence {divergence_bps:.1}bps > {max_divergence_bps}bps — skipping cycle (no actions)"
        ),
        maker::CycleSkip::MissingTouch => {
            // Fail-safe: without a touch we cannot guarantee no-cross pricing.
            eprintln!("⚠️  #{cycle} empty order book on {symbol}; skipping this cycle")
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
