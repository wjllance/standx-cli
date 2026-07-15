//! CLI adapter for normalized deterministic maker replay traces.

use crate::cli::OutputFormat;
use anyhow::{Context, Result};
use serde::Deserialize;
use standx_maker::{
    run_replay, Action, ExecutionCosts, FillRole, MakerConfig, MarketSnapshot, PerformanceFill,
    ReplayCycle, ReplayEvent, ReplayResult, ReplaySettings, RestingQuote,
};
use standx_sdk::models::OrderSide;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum TraceRecord {
    Header {
        schema_version: u32,
        symbol: String,
        git_sha: String,
        config_hash: String,
        seed: u64,
        config: TraceMakerConfig,
        settings: TraceReplaySettings,
    },
    Cycle {
        event_time_ms: i64,
        cycle: u64,
        mark: f64,
        best_bid: Option<f64>,
        best_ask: Option<f64>,
        position: f64,
        #[serde(default)]
        resting: Vec<TraceRestingQuote>,
        #[serde(default)]
        pending_slots: Vec<TraceQuoteSlot>,
        eligible_bid_qty: f64,
        eligible_ask_qty: f64,
    },
    Fill {
        trade_id: u64,
        order_id: u64,
        role: TraceFillRole,
        side: OrderSide,
        price: f64,
        qty: f64,
        mark_at_fill: f64,
        event_time_ms: i64,
        costs: Option<TraceExecutionCosts>,
    },
    Funding {
        event_time_ms: i64,
        cashflow_quote: f64,
    },
    Finish {
        event_time_ms: i64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TraceFillRole {
    PassiveMaker,
    InventoryExit,
}

impl From<TraceFillRole> for FillRole {
    fn from(value: TraceFillRole) -> Self {
        match value {
            TraceFillRole::PassiveMaker => Self::PassiveMaker,
            TraceFillRole::InventoryExit => Self::InventoryExit,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceExecutionCosts {
    fee_quote: f64,
    rebate_quote: f64,
}

impl From<TraceExecutionCosts> for ExecutionCosts {
    fn from(value: TraceExecutionCosts) -> Self {
        Self {
            fee_quote: value.fee_quote,
            rebate_quote: value.rebate_quote,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceMakerConfig {
    spread_bps: f64,
    band_bps: f64,
    level_step_bps: f64,
    refresh_bps: f64,
    levels: u32,
    size: f64,
    max_position: f64,
    skew_bps: f64,
    price_decimals: u32,
    qty_decimals: u32,
    min_order_qty: f64,
}

impl From<TraceMakerConfig> for MakerConfig {
    fn from(value: TraceMakerConfig) -> Self {
        Self {
            spread_bps: value.spread_bps,
            band_bps: value.band_bps,
            level_step_bps: value.level_step_bps,
            refresh_bps: value.refresh_bps,
            levels: value.levels,
            size: value.size,
            max_position: value.max_position,
            skew_bps: value.skew_bps,
            price_decimals: value.price_decimals,
            qty_decimals: value.qty_decimals,
            min_order_qty: value.min_order_qty,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceReplaySettings {
    starting_position: f64,
    starting_mark: f64,
    max_divergence_bps: f64,
    require_full_touch: bool,
    vol_window: usize,
    vol_pause_bps: f64,
    active_exit_enabled: bool,
    inventory_exit_pct: f64,
    inventory_exit_qty: f64,
}

impl From<TraceReplaySettings> for ReplaySettings {
    fn from(value: TraceReplaySettings) -> Self {
        Self {
            starting_position: value.starting_position,
            starting_mark: value.starting_mark,
            max_divergence_bps: value.max_divergence_bps,
            require_full_touch: value.require_full_touch,
            vol_window: value.vol_window,
            vol_pause_bps: value.vol_pause_bps,
            active_exit_enabled: value.active_exit_enabled,
            inventory_exit_pct: value.inventory_exit_pct,
            inventory_exit_qty: value.inventory_exit_qty,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceRestingQuote {
    order_id: Option<String>,
    side: OrderSide,
    level: u32,
    price: f64,
    qty: f64,
    ref_center: f64,
    placed_at_cycle: u64,
}

impl From<TraceRestingQuote> for RestingQuote {
    fn from(value: TraceRestingQuote) -> Self {
        Self {
            order_id: value.order_id,
            side: value.side,
            level: value.level,
            price: value.price,
            qty: value.qty,
            ref_center: value.ref_center,
            placed_at_cycle: value.placed_at_cycle,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceQuoteSlot {
    side: OrderSide,
    level: u32,
}

struct ParsedTrace {
    symbol: String,
    git_sha: String,
    config_hash: String,
    seed: u64,
    config: MakerConfig,
    settings: ReplaySettings,
    events: Vec<ReplayEvent>,
    end_time_ms: i64,
}

pub(super) fn run(path: &Path, output_format: OutputFormat) -> Result<()> {
    let reader: Box<dyn Read> = if path == Path::new("-") {
        Box::new(std::io::stdin())
    } else {
        Box::new(
            std::fs::File::open(path)
                .with_context(|| format!("failed to open replay trace {}", path.display()))?,
        )
    };
    let trace = parse(BufReader::new(reader))?;
    let result = run_replay(
        &trace.config,
        trace.settings,
        &trace.events,
        trace.end_time_ms,
    )?;
    emit(&trace, &result, output_format);
    Ok(())
}

fn parse(reader: impl BufRead) -> Result<ParsedTrace> {
    let mut header = None;
    let mut events = Vec::new();
    let mut finish = None;
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line =
            line.with_context(|| format!("failed to read replay trace line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if finish.is_some() {
            anyhow::bail!("replay trace has a record after finish at line {line_number}");
        }
        let record: TraceRecord = serde_json::from_str(&line)
            .with_context(|| format!("invalid replay trace record at line {line_number}"))?;
        match record {
            TraceRecord::Header {
                schema_version,
                symbol,
                git_sha,
                config_hash,
                seed,
                config,
                settings,
            } => {
                if header.is_some() || !events.is_empty() {
                    anyhow::bail!("replay header must be the first and only header");
                }
                if schema_version != TRACE_SCHEMA_VERSION {
                    anyhow::bail!(
                        "unsupported replay schema version {schema_version}; expected {TRACE_SCHEMA_VERSION}"
                    );
                }
                if symbol.trim().is_empty()
                    || git_sha.trim().is_empty()
                    || config_hash.trim().is_empty()
                {
                    anyhow::bail!("replay header identity fields must be non-empty");
                }
                header = Some((
                    symbol,
                    git_sha,
                    config_hash,
                    seed,
                    config.into(),
                    settings.into(),
                ));
            }
            TraceRecord::Cycle {
                event_time_ms,
                cycle,
                mark,
                best_bid,
                best_ask,
                position,
                resting,
                pending_slots,
                eligible_bid_qty,
                eligible_ask_qty,
            } => {
                require_header(&header, line_number)?;
                events.push(ReplayEvent::Cycle(ReplayCycle {
                    event_time_ms,
                    cycle,
                    market: MarketSnapshot {
                        mark,
                        best_bid,
                        best_ask,
                    },
                    position,
                    resting: resting.into_iter().map(Into::into).collect(),
                    pending_slots: pending_slots
                        .into_iter()
                        .map(|slot| (slot.side, slot.level))
                        .collect(),
                    eligible_bid_qty,
                    eligible_ask_qty,
                }));
            }
            TraceRecord::Fill {
                trade_id,
                order_id,
                role,
                side,
                price,
                qty,
                mark_at_fill,
                event_time_ms,
                costs,
            } => {
                require_header(&header, line_number)?;
                events.push(ReplayEvent::Fill(PerformanceFill {
                    trade_id,
                    order_id,
                    role: role.into(),
                    side,
                    price,
                    qty,
                    mark_at_fill,
                    event_time_ms,
                    costs: costs.map(Into::into),
                }));
            }
            TraceRecord::Funding {
                event_time_ms,
                cashflow_quote,
            } => {
                require_header(&header, line_number)?;
                events.push(ReplayEvent::Funding {
                    event_time_ms,
                    cashflow_quote,
                });
            }
            TraceRecord::Finish { event_time_ms } => {
                require_header(&header, line_number)?;
                finish = Some(event_time_ms);
            }
        }
    }
    let (symbol, git_sha, config_hash, seed, config, settings) =
        header.context("replay trace is missing header")?;
    let end_time_ms = finish.context("replay trace is missing finish")?;
    Ok(ParsedTrace {
        symbol,
        git_sha,
        config_hash,
        seed,
        config,
        settings,
        events,
        end_time_ms,
    })
}

fn require_header<T>(header: &Option<T>, line_number: usize) -> Result<()> {
    if header.is_none() {
        anyhow::bail!("replay record at line {line_number} appears before header");
    }
    Ok(())
}

fn emit(trace: &ParsedTrace, result: &ReplayResult, output_format: OutputFormat) {
    if output_format == OutputFormat::Json {
        for cycle in &result.cycles {
            let actions = cycle
                .plan
                .as_ref()
                .map(|plan| plan.actions.iter().map(action_json).collect::<Vec<_>>());
            println!(
                "{}",
                serde_json::json!({
                    "action": "replay_cycle",
                    "symbol": trace.symbol,
                    "git_sha": trace.git_sha,
                    "config_hash": trace.config_hash,
                    "seed": trace.seed,
                    "event_time_ms": cycle.event_time_ms,
                    "cycle": cycle.cycle,
                    "halted": cycle.preflight.halted,
                    "skip": cycle.preflight.skip.map(|skip| format!("{skip:?}")),
                    "actions": actions,
                })
            );
        }
        println!("{}", summary_json(trace, result));
    } else {
        println!(
            "Replay {} cycles={} passive_fills={} exit_fills={} net_pnl={:.6} uptime={:.2}%",
            trace.symbol,
            result.cycles.len(),
            result.performance.passive_fills,
            result.performance.exit_fills,
            result.performance.net_pnl_quote,
            result.performance.quote_time.two_sided_uptime_pct,
        );
    }
}

fn action_json(action: &Action) -> serde_json::Value {
    match action {
        Action::Place(quote) => serde_json::json!({
            "kind": "place", "side": quote.side, "level": quote.level,
            "price": quote.price, "qty": quote.qty,
        }),
        Action::Cancel {
            order_id,
            side,
            level,
            price,
            reason,
        } => serde_json::json!({
            "kind": "cancel", "order_id": order_id, "side": side,
            "level": level, "price": price, "reason": reason.as_str(),
        }),
        Action::Hold {
            side,
            level,
            price,
            age_cycles,
            drift_bps,
        } => serde_json::json!({
            "kind": "hold", "side": side, "level": level, "price": price,
            "age_cycles": age_cycles, "drift_bps": drift_bps,
        }),
    }
}

fn summary_json(trace: &ParsedTrace, result: &ReplayResult) -> serde_json::Value {
    let performance = &result.performance;
    let markouts = performance
        .markouts
        .iter()
        .map(|markout| {
            serde_json::json!({
                "window_ms": markout.window_ms,
                "samples": markout.samples,
                "pending": markout.pending,
                "unavailable": markout.unavailable,
                "qty": markout.qty,
                "quote_pnl": markout.quote_pnl,
                "avg_bps": markout.avg_bps,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "action": "replay_summary",
        "schema_version": TRACE_SCHEMA_VERSION,
        "symbol": trace.symbol,
        "git_sha": trace.git_sha,
        "config_hash": trace.config_hash,
        "seed": trace.seed,
        "cycles": result.cycles.len(),
        "passive_fills": performance.passive_fills,
        "passive_qty": performance.passive_qty,
        "passive_cashflow_quote": performance.passive_cashflow_quote,
        "passive_capture_bps": performance.passive_capture_bps,
        "exit_fills": performance.exit_fills,
        "exit_qty": performance.exit_qty,
        "exit_cashflow_quote": performance.exit_cashflow_quote,
        "gross_spread_quote": performance.gross_spread_quote,
        "fee_quote": performance.fee_quote,
        "rebate_quote": performance.rebate_quote,
        "execution_costs_unavailable": performance.execution_costs_unavailable,
        "funding_quote": performance.funding_quote,
        "funding_available": performance.funding_available,
        "net_pnl_complete": performance.net_pnl_complete,
        "exit_cost_quote": performance.exit_cost_quote,
        "inventory_mtm_change_quote": performance.inventory_mtm_change_quote,
        "net_pnl_quote": performance.net_pnl_quote,
        "position": performance.position,
        "markouts": markouts,
        "time_weighted_uptime_pct": performance.quote_time.two_sided_uptime_pct,
        "observed_ms": performance.quote_time.observed_ms,
        "two_sided_ms": performance.quote_time.two_sided_ms,
        "eligible_bid_qty_ms": performance.quote_time.eligible_bid_qty_ms,
        "eligible_ask_qty_ms": performance.quote_time.eligible_ask_qty_ms,
        "eligible_total_qty_ms": performance.quote_time.eligible_total_qty_ms,
        "inventory_observed_ms": performance.inventory_time.observed_ms,
        "inventory_nonzero_ms": performance.inventory_time.nonzero_ms,
        "inventory_abs_qty_ms": performance.inventory_time.abs_qty_ms,
        "inventory_avg_abs_qty": performance.inventory_time.avg_abs_qty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE: &str = r#"{"type":"header","schema_version":1,"symbol":"BTC-USD","git_sha":"abc","config_hash":"def","seed":7,"config":{"spread_bps":5.0,"band_bps":20.0,"level_step_bps":2.0,"refresh_bps":3.0,"levels":1,"size":1.0,"max_position":10.0,"skew_bps":0.0,"price_decimals":2,"qty_decimals":2,"min_order_qty":0.01},"settings":{"starting_position":0.0,"starting_mark":100.0,"max_divergence_bps":25.0,"require_full_touch":true,"vol_window":12,"vol_pause_bps":0.0,"active_exit_enabled":false,"inventory_exit_pct":0.0,"inventory_exit_qty":0.0}}
{"type":"cycle","event_time_ms":0,"cycle":0,"mark":100.0,"best_bid":99.99,"best_ask":100.01,"position":0.0,"eligible_bid_qty":1.0,"eligible_ask_qty":1.0}
{"type":"fill","trade_id":1,"order_id":10,"role":"passive_maker","side":"buy","price":99.95,"qty":1.0,"mark_at_fill":100.0,"event_time_ms":0,"costs":{"fee_quote":0.0,"rebate_quote":0.0}}
{"type":"cycle","event_time_ms":1000,"cycle":1,"mark":100.1,"best_bid":100.09,"best_ask":100.11,"position":1.0,"eligible_bid_qty":1.0,"eligible_ask_qty":1.0}
{"type":"finish","event_time_ms":1000}
"#;

    #[test]
    fn parses_and_replays_normalized_trace_without_external_state() {
        let trace = parse(BufReader::new(TRACE.as_bytes())).unwrap();
        let first = run_replay(
            &trace.config,
            trace.settings,
            &trace.events,
            trace.end_time_ms,
        )
        .unwrap();
        let second = run_replay(
            &trace.config,
            trace.settings,
            &trace.events,
            trace.end_time_ms,
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.cycles.len(), 2);
        assert_eq!(first.performance.passive_fills, 1);
    }

    #[test]
    fn rejects_unknown_fields_and_records_after_finish() {
        let unknown = TRACE.replace("\"seed\":7", "\"seed\":7,\"surprise\":true");
        assert!(parse(BufReader::new(unknown.as_bytes())).is_err());
        let trailing =
            format!("{TRACE}{{\"type\":\"funding\",\"event_time_ms\":2,\"cashflow_quote\":0.1}}\n");
        assert!(parse(BufReader::new(trailing.as_bytes())).is_err());
    }
}
