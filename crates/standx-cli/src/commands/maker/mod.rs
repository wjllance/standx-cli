use crate::cli::*;
use anyhow::Result;
use standx_sdk::auth::Credentials;
use standx_sdk::client::StandXClient;
use standx_sdk::error::Error as StandxError;
use standx_sdk::models::OrderSide;
use std::collections::HashMap;
use std::time::Duration;
use tokio::signal;

mod cycle;
mod feed;
mod output;

use cycle::{cancel_all_with_retry, maker_cycle};
use feed::{market_snapshot, spawn_market_feed};

// ============================================================================
// Maker bot (SIP-5A community maker yield)
// ============================================================================

/// Env var gating live order placement. The live path ships code-complete but
/// locked until it has been supervised-tested against production.
const LIVE_MAKER_ENV: &str = "STANDX_ENABLE_LIVE_MAKER";

/// Why the maker loop stopped.
enum MakerExit {
    CtrlC,
    /// Too many consecutive API errors — fail safe, not open.
    FailSafe(String),
}

/// Pending place awaiting order-id adoption (live mode): create_order only
/// returns a request id, so new open orders are matched back to recent
/// places by (side, price, qty) on the next cycle.
struct PendingPlace {
    side: OrderSide,
    price: f64,
    qty: f64,
    level: u32,
    ref_center: f64,
    cycle: u64,
}

/// A business rejection from the venue: the exchange responded with a
/// definite "no" (post-only would-cross, order not found, insufficient
/// margin, …). These are expected during normal quoting and must NOT trip
/// the fail-safe — that is reserved for transient failures (network, 5xx,
/// rate limit) that signal we can no longer talk to the exchange, which
/// keep their `retryable` flag and propagate as cycle errors.
fn is_order_rejection(e: &StandxError) -> bool {
    matches!(
        e,
        StandxError::Api {
            retryable: false,
            ..
        }
    )
}

/// Whether an open order's remaining `open_qty` plausibly belongs to a place
/// we made for `placed_qty`. A partial fill leaves a positive remainder no
/// larger than what we placed; anything bigger is somebody else's order.
/// Tolerating the shrink is what keeps a partially-filled order adopted
/// (and thus HELD) instead of cancelled as an unknown order.
fn open_qty_adopts(open_qty: f64, placed_qty: f64) -> bool {
    open_qty > 0.0 && open_qty <= placed_qty * (1.0 + 1e-6)
}

/// Handle maker commands
pub async fn handle_maker(
    command: MakerCommands,
    output_format: OutputFormat,
    verbose: bool,
) -> Result<()> {
    match command {
        MakerCommands::Run {
            symbol,
            spread_bps,
            band_bps,
            size,
            levels,
            level_step_bps,
            refresh_bps,
            interval,
            max_position,
            skew_bps,
            max_divergence_bps,
            no_ws,
            live,
        } => {
            run_maker(
                symbol,
                MakerRunArgs {
                    spread_bps,
                    band_bps,
                    size,
                    levels,
                    level_step_bps,
                    refresh_bps,
                    interval,
                    max_position,
                    skew_bps,
                    max_divergence_bps,
                    no_ws,
                    live,
                    verbose,
                },
                output_format,
            )
            .await
        }
    }
}

struct MakerRunArgs {
    spread_bps: f64,
    band_bps: f64,
    size: f64,
    levels: u32,
    level_step_bps: f64,
    refresh_bps: f64,
    interval: u64,
    max_position: f64,
    skew_bps: f64,
    max_divergence_bps: f64,
    no_ws: bool,
    live: bool,
    verbose: bool,
}

async fn run_maker(symbol: String, args: MakerRunArgs, output_format: OutputFormat) -> Result<()> {
    use standx_sdk::maker::{self, MakerConfig, RestingQuote};

    let client = StandXClient::new()?;

    // ---- Startup: symbol metadata + invariants (fail fast) ----
    let infos = client.get_symbol_info().await?;
    let info = infos
        .iter()
        .find(|i| i.symbol.eq_ignore_ascii_case(&symbol))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown symbol '{}'. Available: {}",
                symbol,
                infos
                    .iter()
                    .map(|i| i.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    if info.status != "trading" {
        return Err(anyhow::anyhow!(
            "Symbol {} is not trading (status: {})",
            info.symbol,
            info.status
        ));
    }
    let symbol = info.symbol.clone(); // canonical casing

    let min_order_qty: f64 = info.min_order_qty.parse().unwrap_or(0.0);
    let cfg = MakerConfig {
        spread_bps: args.spread_bps,
        band_bps: args.band_bps,
        level_step_bps: args.level_step_bps,
        refresh_bps: args.refresh_bps,
        levels: args.levels.max(1),
        size: args.size,
        max_position: args.max_position,
        skew_bps: args.skew_bps,
        price_decimals: info.price_tick_decimals,
        qty_decimals: info.qty_tick_decimals,
        min_order_qty,
    };

    if cfg.spread_bps <= 0.0 {
        return Err(anyhow::anyhow!("--spread-bps must be > 0"));
    }
    if cfg.skew_bps < 0.0 {
        return Err(anyhow::anyhow!("--skew-bps must be >= 0"));
    }
    if cfg.band_bps <= cfg.spread_bps {
        return Err(anyhow::anyhow!(
            "--band-bps ({}) must be greater than --spread-bps ({}): quotes clamped to the band edge would sit exactly at the boundary",
            cfg.band_bps,
            cfg.spread_bps
        ));
    }
    let rounded_size = maker::round_to_decimals(cfg.size, cfg.qty_decimals);
    if rounded_size < cfg.min_order_qty || rounded_size <= 0.0 {
        return Err(anyhow::anyhow!(
            "--size {} (rounded to {} at {} decimals) is below min order qty {} for {}",
            cfg.size,
            rounded_size,
            cfg.qty_decimals,
            cfg.min_order_qty,
            symbol
        ));
    }
    if cfg.refresh_bps >= cfg.spread_bps {
        eprintln!(
            "⚠️  --refresh-bps ({}) >= --spread-bps ({}): quotes will be held through large drifts",
            cfg.refresh_bps, cfg.spread_bps
        );
    }
    if cfg.levels > 1
        && cfg.spread_bps + (cfg.levels - 1) as f64 * cfg.level_step_bps >= cfg.band_bps
    {
        eprintln!("⚠️  outer quote levels exceed the band and will be clamped/collapsed");
    }

    // ---- Live gating & clean start ----
    if args.live {
        if std::env::var(LIVE_MAKER_ENV).ok().as_deref() != Some("1") {
            return Err(anyhow::anyhow!(
                "live mode not yet enabled: it has not been supervised-tested against production. Set {}=1 to unlock (at your own risk).",
                LIVE_MAKER_ENV
            ));
        }
        let creds = Credentials::load()?;
        if creds.is_expired() {
            return Err(anyhow::anyhow!(
                "Credentials expired. Run 'standx auth login' first."
            ));
        }
        if creds.private_key.is_empty() {
            return Err(anyhow::anyhow!(
                "Live mode requires a private key for order signing. Run 'standx auth login' with --private-key."
            ));
        }
        // Start from a clean book so reconciliation isn't confused by
        // leftovers from a previous run. The bot owns ALL orders on this
        // symbol while running.
        client.cancel_all_orders(&symbol).await?;
    }

    let mode = if args.live { "LIVE" } else { "PAPER" };
    if output_format == OutputFormat::Table {
        println!("┌──────────────────────────────────────────────────────────┐");
        println!("│ standx maker — {} mode on {}", mode, symbol);
        println!(
            "│ spread {}bps | band {}bps | refresh {}bps | {} level(s)",
            cfg.spread_bps, cfg.band_bps, cfg.refresh_bps, cfg.levels
        );
        println!(
            "│ size {} | max-position {} | interval {}s",
            cfg.size, cfg.max_position, args.interval
        );
        if cfg.skew_bps > 0.0 {
            println!(
                "│ inventory skew {}bps (live only; paper holds no position)",
                cfg.skew_bps
            );
        }
        println!(
            "│ ticks: price {}dp, qty {}dp | min qty {}",
            cfg.price_decimals, cfg.qty_decimals, cfg.min_order_qty
        );
        if !args.live {
            println!("│ paper mode: no real orders; fills are simulated when the");
            println!("│ touch crosses a quote, so position & skew move. --live for real.");
        } else {
            println!(
                "│ ⚠️  LIVE: the bot manages ALL orders on {} — manual",
                symbol
            );
            println!("│ orders on this symbol will be cancelled as stale.");
        }
        if args.no_ws {
            println!("│ feed: REST polling (--no-ws)");
        } else {
            println!(
                "│ feed: websocket (REST fallback) | divergence guard {}bps",
                args.max_divergence_bps
            );
        }
        println!("│ Ctrl+C to stop (cancels all resting orders on exit)");
        println!("└──────────────────────────────────────────────────────────┘");
    }

    // ---- Market feed (WS primary, REST fallback) ----
    let (feed, mut updates, feed_handle) = if args.no_ws {
        (None, None, None)
    } else {
        let (state, rx, handle) = spawn_market_feed(symbol.clone(), args.verbose);
        (Some(state), Some(rx), Some(handle))
    };

    // ---- Loop state ----
    let mut cycle: u64 = 0;
    let mut resting: Vec<RestingQuote> = Vec::new(); // paper-mode book
    let mut adopted: HashMap<String, (u32, f64, u64)> = HashMap::new(); // id -> (level, ref_mark, cycle)
    let mut pending: Vec<PendingPlace> = Vec::new();
    let mut consecutive_errors: u32 = 0;
    let mut total_places: u64 = 0;
    let mut total_cancels: u64 = 0;
    let mut total_holds: u64 = 0;
    let mut total_fills: u64 = 0;
    let mut sim_position: f64 = 0.0; // paper-mode simulated inventory
    let mut stats = maker::MakerStats::default();
    let mut last_mark: Option<f64> = None;
    let mut last_src: Option<&'static str> = None;

    let exit = 'main: loop {
        // Work phase raced against Ctrl+C so a slow API call can be
        // interrupted (mirrors run_watch_loop).
        let work = async {
            let (mark, best_bid, best_ask, src) =
                market_snapshot(&client, &symbol, feed.as_ref()).await?;
            let (places, cancels, holds, fills) = maker_cycle(
                &client,
                &symbol,
                &cfg,
                args.live,
                cycle,
                mark,
                best_bid,
                best_ask,
                args.max_divergence_bps,
                &mut resting,
                &mut adopted,
                &mut pending,
                &mut sim_position,
                &mut stats,
                output_format,
            )
            .await?;
            Ok::<_, anyhow::Error>((places, cancels, holds, fills, mark, src))
        };
        let cycle_result = tokio::select! {
            _ = signal::ctrl_c() => break MakerExit::CtrlC,
            result = work => result,
        };

        match cycle_result {
            Ok((places, cancels, holds, fills, mark, src)) => {
                consecutive_errors = 0;
                total_places += places;
                total_cancels += cancels;
                total_holds += holds;
                total_fills += fills;
                last_mark = Some(mark);
                if !args.no_ws && last_src != Some(src) {
                    match src {
                        "ws" => eprintln!("✅ market feed: websocket live"),
                        _ => eprintln!(
                            "⚠️  market feed: REST fallback (websocket warming up or stale)"
                        ),
                    }
                    last_src = Some(src);
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("⚠️  maker cycle failed ({}/3): {}", consecutive_errors, e);
                if consecutive_errors >= 3 {
                    break MakerExit::FailSafe(e.to_string());
                }
            }
        }

        cycle += 1;

        // Sleep until the next cycle, but wake early when the cached mark
        // has already drifted beyond refresh_bps — the quotes would be
        // re-quoted anyway, so reacting now shrinks the pick-off window
        // without adding flicker. min-gap of 1s bounds the API rate.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(args.interval);
        let min_gap = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let update = async {
                match updates.as_mut() {
                    Some(rx) => rx.changed().await.is_ok(),
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = signal::ctrl_c() => break 'main MakerExit::CtrlC,
                _ = tokio::time::sleep_until(deadline) => break,
                ok = update => {
                    if !ok {
                        // Feed task gone: fall back to plain interval waits.
                        updates = None;
                        continue;
                    }
                    if tokio::time::Instant::now() < min_gap {
                        continue;
                    }
                    let (Some(feed), Some(prev)) = (feed.as_ref(), last_mark) else {
                        continue;
                    };
                    let drifted = {
                        let s = feed.read().await;
                        s.mark
                            .is_some_and(|m| maker::bps_diff(m, prev) > cfg.refresh_bps)
                    };
                    if drifted {
                        break; // early re-quote cycle
                    }
                }
            }
        }
    };

    // ---- Cleanup on ALL exit paths ----
    if let Some(handle) = feed_handle {
        handle.abort();
    }
    if output_format == OutputFormat::Table {
        println!(
            "\n👋 Stopping maker (ran {} cycles: {} places, {} cancels, {} holds)",
            cycle, total_places, total_cancels, total_holds
        );
        let pnl_note = match last_mark {
            Some(m) => format!(" | PnL {:+.2} (mark-to-market)", stats.mark_to_market(m)),
            None => String::new(),
        };
        println!(
            "   {} fills | uptime {:.0}% | max pos {} | avg capture {:.1}bps{}",
            total_fills,
            stats.uptime_pct(),
            maker::format_decimals(stats.max_abs_position, cfg.qty_decimals),
            stats.avg_spread_capture_bps(),
            pnl_note
        );
        if !args.live {
            println!(
                "   paper sim: ending position {}",
                maker::format_decimals(sim_position, cfg.qty_decimals)
            );
        }
    }
    if args.live {
        cancel_all_with_retry(&client, &symbol, 3).await?;
    }

    match exit {
        MakerExit::CtrlC => Ok(()),
        MakerExit::FailSafe(e) => Err(anyhow::anyhow!(
            "maker stopped after 3 consecutive errors (fail-safe): {}",
            e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_rejection_not_fail_safe() {
        // Post-only would-cross / order-not-found: exchange said no.
        assert!(is_order_rejection(&StandxError::Api {
            code: 400,
            message: "post-only would cross".into(),
            endpoint: None,
            retryable: false,
        }));
        // 5xx from the venue: transient → counts toward fail-safe.
        assert!(!is_order_rejection(&StandxError::Api {
            code: 502,
            message: "bad gateway".into(),
            endpoint: None,
            retryable: true,
        }));
        // Network layer: transient → counts toward fail-safe.
        assert!(!is_order_rejection(&StandxError::Http {
            code: 0,
            message: "connection reset".into(),
            retryable: Some(true),
        }));
    }

    #[test]
    fn partial_fill_stays_adopted() {
        // Full remainder adopts.
        assert!(open_qty_adopts(0.01, 0.01));
        // Partial remainder (half filled) still adopts.
        assert!(open_qty_adopts(0.005, 0.01));
        // Tiny remainder adopts.
        assert!(open_qty_adopts(0.0001, 0.01));
        // Zero / fully filled does not adopt (no open order to match).
        assert!(!open_qty_adopts(0.0, 0.01));
        // Larger than placed is someone else's order.
        assert!(!open_qty_adopts(0.02, 0.01));
        // Float slop just under the placed qty is tolerated.
        assert!(open_qty_adopts(0.01 + 1e-9, 0.01));
    }
}
