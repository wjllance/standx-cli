use super::model::PendingPlace;
use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker::{MakerConfig, MakerLedger, MakerStats, RestingQuote, VolBreaker};
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Balance, Order, Position, Trade};
use standx_sdk::order_response::OrderResponseHealth;
use std::collections::HashMap;

pub(super) struct CycleRequest<'a> {
    pub(super) client: &'a StandXClient,
    pub(super) symbol: &'a str,
    pub(super) cfg: &'a MakerConfig,
    pub(super) live: bool,
    pub(super) cycle: u64,
    pub(super) mark: f64,
    pub(super) best_bid: Option<f64>,
    pub(super) best_ask: Option<f64>,
    pub(super) max_divergence_bps: f64,
    pub(super) inventory_exit_pct: f64,
    pub(super) inventory_exit_qty: f64,
    pub(super) session_started_at: i64,
    pub(super) run_order_prefix: &'a str,
    pub(super) starting_position: f64,
    pub(super) output_format: OutputFormat,
    pub(super) order_response_health: Option<&'a OrderResponseHealth>,
}

pub(super) struct CycleState<'a> {
    pub(super) resting: &'a mut Vec<RestingQuote>,
    pub(super) adopted: &'a mut HashMap<String, (u32, f64, u64)>,
    pub(super) pending: &'a mut Vec<PendingPlace>,
    pub(super) inventory_exit_pending: &'a mut bool,
    pub(super) ledger: &'a mut MakerLedger,
    pub(super) sim_position: &'a mut f64,
    pub(super) stats: &'a mut MakerStats,
    pub(super) breaker: &'a mut VolBreaker,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct CycleResult {
    pub(super) places: u64,
    pub(super) cancels: u64,
    pub(super) holds: u64,
    pub(super) fills: u64,
    /// Latest account snapshot (live mode only; `None` in paper mode).
    pub(super) balance: Option<Balance>,
}

pub(super) struct VenueSnapshot {
    pub(super) open_orders: Vec<Order>,
    pub(super) filled_orders: Vec<Order>,
    pub(super) trades: Vec<Trade>,
    pub(super) positions: Vec<Position>,
    pub(super) balance: Balance,
}

pub(super) async fn fetch_snapshot(
    client: &StandXClient,
    symbol: &str,
    session_started_at: i64,
    now: i64,
) -> Result<VenueSnapshot> {
    let (open_orders, filled_orders, trades, positions, balance) = tokio::join!(
        client.get_open_orders(Some(symbol)),
        client.get_order_history(Some(symbol), Some(100)),
        client.get_user_trades(symbol, session_started_at, now, Some(500)),
        client.get_positions(Some(symbol)),
        client.get_balance(),
    );
    Ok(VenueSnapshot {
        open_orders: open_orders?,
        filled_orders: filled_orders?,
        trades: trades?,
        positions: positions?,
        balance: balance?,
    })
}
