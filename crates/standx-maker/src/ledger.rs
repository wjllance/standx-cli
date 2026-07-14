//! Deterministic current-run fill accounting.
//!
//! Transport adapters validate and normalize venue payloads before calling this
//! module. The ledger then owns order adoption, WS/REST deduplication,
//! cumulative-fill deltas, session stats, and expected position.

use crate::{is_current_run_client_order_id, MakerStats};
use standx_sdk::models::OrderSide;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct MakerFill {
    pub side: OrderSide,
    pub price: f64,
    pub qty: f64,
    pub trade_id: Option<u64>,
    pub order_id: Option<u64>,
    pub trade_ts: Option<String>,
    pub origin: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CumulativeFill<'a> {
    pub order_id: u64,
    pub side: OrderSide,
    pub qty: f64,
    pub notional: f64,
    pub mark: f64,
    pub origin: &'static str,
    pub trade_id: Option<u64>,
    pub trade_ts: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct RestFill<'a> {
    pub trade_id: u64,
    pub order_id: u64,
    pub side: OrderSide,
    pub price: f64,
    pub qty: f64,
    pub mark: f64,
    pub trade_ts: &'a str,
}

#[derive(Clone, Copy, Debug, Default)]
struct FillTotals {
    qty: f64,
    notional: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LedgerError {
    MissingTradeId { order_id: u64 },
    InvalidCumulativeFill { order_id: u64, qty: f64, price: f64 },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTradeId { order_id } => {
                write!(
                    formatter,
                    "maker fill for order {order_id} has no stable trade ID"
                )
            }
            Self::InvalidCumulativeFill {
                order_id,
                qty,
                price,
            } => write!(
                formatter,
                "invalid cumulative fill for maker order {order_id}: qty={qty}, price={price}"
            ),
        }
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug)]
pub struct MakerLedger {
    pub expected_position: f64,
    pub maker_order_ids: HashSet<u64>,
    pub exit_order_ids: HashSet<u64>,
    seen_fill_ids: HashSet<u64>,
    accounted: HashMap<u64, FillTotals>,
    rest_seen: HashMap<u64, FillTotals>,
}

impl MakerLedger {
    pub fn new(starting_position: f64) -> Self {
        Self {
            expected_position: starting_position,
            maker_order_ids: HashSet::new(),
            exit_order_ids: HashSet::new(),
            seen_fill_ids: HashSet::new(),
            accounted: HashMap::new(),
            rest_seen: HashMap::new(),
        }
    }

    /// Adopt an order only when its client ID belongs to this run.
    pub fn adopt_order(
        &mut self,
        order_id: u64,
        client_order_id: Option<&str>,
        run_order_prefix: &str,
    ) -> bool {
        if !is_current_run_client_order_id(client_order_id, run_order_prefix) {
            return false;
        }
        self.maker_order_ids.insert(order_id);
        if client_order_id.is_some_and(|id| id.starts_with(&format!("{run_order_prefix}x"))) {
            self.exit_order_ids.insert(order_id);
        }
        true
    }

    pub fn is_exit_order(&self, order_id: u64) -> bool {
        self.exit_order_ids.contains(&order_id)
    }

    pub fn record_cumulative_fill(
        &mut self,
        fill: CumulativeFill<'_>,
        stats: &mut MakerStats,
    ) -> Result<Option<MakerFill>, LedgerError> {
        let previous = self
            .accounted
            .get(&fill.order_id)
            .copied()
            .unwrap_or_default();
        let qty = fill.qty - previous.qty;
        if qty <= 1e-12 {
            return Ok(None);
        }
        let notional = fill.notional - previous.notional;
        let price = notional / qty;
        if !qty.is_finite() || !price.is_finite() || qty <= 0.0 || price <= 0.0 {
            return Err(LedgerError::InvalidCumulativeFill {
                order_id: fill.order_id,
                qty,
                price,
            });
        }
        stats.record_fill(fill.side, price, qty, fill.mark);
        self.expected_position += match fill.side {
            OrderSide::Buy => qty,
            OrderSide::Sell => -qty,
        };
        stats.observe_position(self.expected_position);
        self.accounted.insert(
            fill.order_id,
            FillTotals {
                qty: fill.qty,
                notional: fill.notional,
            },
        );
        Ok(Some(MakerFill {
            side: fill.side,
            price,
            qty,
            trade_id: fill.trade_id,
            order_id: Some(fill.order_id),
            trade_ts: fill.trade_ts.map(str::to_owned),
            origin: fill.origin,
        }))
    }

    /// Record a validated REST trade. The running per-order REST total makes
    /// this commute with cumulative order-stream updates.
    pub fn record_rest_fill(
        &mut self,
        fill: RestFill<'_>,
        stats: &mut MakerStats,
    ) -> Result<Option<MakerFill>, LedgerError> {
        if !self.maker_order_ids.contains(&fill.order_id) {
            return Ok(None);
        }
        if fill.trade_id == 0 {
            return Err(LedgerError::MissingTradeId {
                order_id: fill.order_id,
            });
        }
        if !self.seen_fill_ids.insert(fill.trade_id) {
            return Ok(None);
        }
        let cumulative = {
            let totals = self.rest_seen.entry(fill.order_id).or_default();
            totals.qty += fill.qty;
            totals.notional += fill.qty * fill.price;
            *totals
        };
        self.record_cumulative_fill(
            CumulativeFill {
                order_id: fill.order_id,
                side: fill.side,
                qty: cumulative.qty,
                notional: cumulative.notional,
                mark: fill.mark,
                origin: "current_run_rest_trade",
                trade_id: Some(fill.trade_id),
                trade_ts: Some(fill.trade_ts),
            },
            stats,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_and_rest_fills_account_only_their_cumulative_delta() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001b0"), "sxmk-run-"));

        let ws = ledger
            .record_cumulative_fill(
                CumulativeFill {
                    order_id: 7,
                    side: OrderSide::Buy,
                    qty: 0.2,
                    notional: 20.0,
                    mark: 100.0,
                    origin: "current_run_ws_order",
                    trade_id: None,
                    trade_ts: Some("2026-07-13T00:00:00Z"),
                },
                &mut stats,
            )
            .unwrap();
        assert!(ws.is_some());
        assert!(ledger
            .record_rest_fill(
                RestFill {
                    trade_id: 1,
                    order_id: 7,
                    side: OrderSide::Buy,
                    price: 100.0,
                    qty: 0.2,
                    mark: 100.0,
                    trade_ts: "2026-07-13T00:00:00Z",
                },
                &mut stats,
            )
            .unwrap()
            .is_none());
        assert_eq!(stats.fills(), 1);
        assert!((ledger.expected_position - 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }

    #[test]
    fn rest_then_ws_fill_updates_cash_and_position_once() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001b0"), "sxmk-run-"));

        assert!(ledger
            .record_rest_fill(
                RestFill {
                    trade_id: 1,
                    order_id: 7,
                    side: OrderSide::Sell,
                    price: 100.0,
                    qty: 0.2,
                    mark: 100.0,
                    trade_ts: "2026-07-13T00:00:00Z",
                },
                &mut stats,
            )
            .unwrap()
            .is_some());
        assert!(ledger
            .record_cumulative_fill(
                CumulativeFill {
                    order_id: 7,
                    side: OrderSide::Sell,
                    qty: 0.2,
                    notional: 20.0,
                    mark: 100.0,
                    origin: "current_run_ws_order",
                    trade_id: None,
                    trade_ts: Some("2026-07-13T00:00:00Z"),
                },
                &mut stats,
            )
            .unwrap()
            .is_none());

        assert_eq!(stats.fills(), 1);
        assert!((stats.cash - 20.0).abs() < 1e-12);
        assert!((ledger.expected_position + 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }

    #[test]
    fn buffered_inventory_exit_fill_keeps_round_trip_pnl_flat_to_notional() {
        for (sell_price, buy_price, expected_pnl) in
            [(57.78, 57.84, -0.012), (58.02, 58.10, -0.016)]
        {
            let mut ledger = MakerLedger::new(0.0);
            let mut stats = MakerStats::default();
            assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001a0"), "sxmk-run-"));
            assert!(ledger.adopt_order(8, Some("sxmk-run-x00000002b0"), "sxmk-run-"));

            ledger
                .record_cumulative_fill(
                    CumulativeFill {
                        order_id: 7,
                        side: OrderSide::Sell,
                        qty: 0.2,
                        notional: sell_price * 0.2,
                        mark: sell_price,
                        origin: "current_run_ws_order",
                        trade_id: None,
                        trade_ts: Some("2026-07-13T14:22:01Z"),
                    },
                    &mut stats,
                )
                .unwrap();
            stats.end_cycle(ledger.expected_position, false);

            ledger
                .record_cumulative_fill(
                    CumulativeFill {
                        order_id: 8,
                        side: OrderSide::Buy,
                        qty: 0.2,
                        notional: buy_price * 0.2,
                        mark: buy_price,
                        origin: "current_run_ws_order",
                        trade_id: None,
                        trade_ts: Some("2026-07-13T14:22:05Z"),
                    },
                    &mut stats,
                )
                .unwrap();

            assert!(ledger.expected_position.abs() < 1e-12);
            assert!(stats.position().abs() < 1e-12);
            assert!((stats.pnl(ledger.expected_position, buy_price) - expected_pnl).abs() < 1e-12);
            assert!(stats.pnl(ledger.expected_position, buy_price) > -4.0);
        }
    }

    #[test]
    fn partial_cumulative_fills_update_position_once_per_delta() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001b0"), "sxmk-run-"));

        for (qty, notional) in [(0.1, 10.0), (0.2, 20.0), (0.2, 20.0)] {
            ledger
                .record_cumulative_fill(
                    CumulativeFill {
                        order_id: 7,
                        side: OrderSide::Buy,
                        qty,
                        notional,
                        mark: 100.0,
                        origin: "current_run_ws_order",
                        trade_id: None,
                        trade_ts: Some("2026-07-13T00:00:00Z"),
                    },
                    &mut stats,
                )
                .unwrap();
        }

        assert_eq!(stats.fills(), 2);
        assert!((stats.cash + 20.0).abs() < 1e-12);
        assert!((ledger.expected_position - 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }
}
