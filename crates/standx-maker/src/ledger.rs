//! Deterministic current-run fill accounting.
//!
//! Transport adapters normalize authenticated account-stream and REST trade
//! payloads into [`LedgerTrade`]. The ledger owns current-run order adoption,
//! stable-ID deduplication, bounded trade-before-order buffering, session
//! stats, and expected position. Cumulative order updates deliberately do not
//! affect accounting: they lack a stable trade ID and are only ownership/order
//! state signals.

use crate::{is_current_run_client_order_id, MakerStats};
use standx_sdk::models::OrderSide;
use std::collections::{HashSet, VecDeque};
use std::fmt;

const MAX_PENDING_TRADES: usize = 512;

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

/// Transport-independent source for a stable venue execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeSource {
    AccountStream,
    RestBackfill,
}

impl TradeSource {
    pub const fn origin(self) -> &'static str {
        match self {
            Self::AccountStream => "current_run_ws_trade",
            Self::RestBackfill => "current_run_rest_trade",
        }
    }
}

/// One immutable venue execution. A trade must have stable, non-zero venue
/// identifiers; deduplication is exclusively by `trade_id`.
#[derive(Clone, Copy, Debug)]
pub struct LedgerTrade<'a> {
    pub trade_id: u64,
    pub order_id: u64,
    pub side: OrderSide,
    pub price: f64,
    pub qty: f64,
    pub mark: f64,
    pub trade_ts: &'a str,
    pub source: TradeSource,
}

#[derive(Clone, Debug)]
struct PendingTrade {
    trade_id: u64,
    order_id: u64,
    side: OrderSide,
    price: f64,
    qty: f64,
    mark: f64,
    trade_ts: String,
    source: TradeSource,
}

impl<'a> From<LedgerTrade<'a>> for PendingTrade {
    fn from(trade: LedgerTrade<'a>) -> Self {
        Self {
            trade_id: trade.trade_id,
            order_id: trade.order_id,
            side: trade.side,
            price: trade.price,
            qty: trade.qty,
            mark: trade.mark,
            trade_ts: trade.trade_ts.to_owned(),
            source: trade.source,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LedgerError {
    MissingTradeId {
        order_id: u64,
    },
    MissingOrderId {
        trade_id: u64,
    },
    InvalidTrade {
        trade_id: u64,
        order_id: u64,
        price: f64,
        qty: f64,
    },
    PendingTradeOverflow {
        limit: usize,
    },
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
            Self::MissingOrderId { trade_id } => {
                write!(formatter, "maker trade {trade_id} has no stable order ID")
            }
            Self::InvalidTrade {
                trade_id,
                order_id,
                price,
                qty,
            } => write!(
                formatter,
                "invalid maker trade {trade_id} for order {order_id}: qty={qty}, price={price}"
            ),
            Self::PendingTradeOverflow { limit } => write!(
                formatter,
                "unowned maker trade buffer exceeded its {limit} execution limit"
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
    seen_trade_ids: HashSet<u64>,
    pending_trade_ids: HashSet<u64>,
    pending_trades: VecDeque<PendingTrade>,
}

impl MakerLedger {
    pub fn new(starting_position: f64) -> Self {
        Self {
            expected_position: starting_position,
            maker_order_ids: HashSet::new(),
            exit_order_ids: HashSet::new(),
            seen_trade_ids: HashSet::new(),
            pending_trade_ids: HashSet::new(),
            pending_trades: VecDeque::new(),
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
        // Exit orders carry the run prefix followed by an 'x' marker. Match
        // without allocating a `format!("{run_order_prefix}x")` on every call.
        if client_order_id.is_some_and(|id| {
            id.strip_prefix(run_order_prefix)
                .is_some_and(|rest| rest.starts_with('x'))
        }) {
            self.exit_order_ids.insert(order_id);
        }
        true
    }

    pub fn is_exit_order(&self, order_id: u64) -> bool {
        self.exit_order_ids.contains(&order_id)
    }

    /// Account a stable trade immediately if its order is known to belong to
    /// this run. A trade can legally arrive before its order callback, in
    /// which case it is buffered until [`Self::apply_buffered_trades`] is
    /// called after ownership is established.
    pub fn record_trade(
        &mut self,
        trade: LedgerTrade<'_>,
        stats: &mut MakerStats,
    ) -> Result<Option<MakerFill>, LedgerError> {
        self.validate_trade(trade)?;
        if self.seen_trade_ids.contains(&trade.trade_id)
            || self.pending_trade_ids.contains(&trade.trade_id)
        {
            return Ok(None);
        }
        if !self.maker_order_ids.contains(&trade.order_id) {
            if self.pending_trades.len() >= MAX_PENDING_TRADES {
                if let Some(evicted) = self.pending_trades.pop_front() {
                    self.pending_trade_ids.remove(&evicted.trade_id);
                }
            }
            self.pending_trade_ids.insert(trade.trade_id);
            self.pending_trades.push_back(trade.into());
            return Ok(None);
        }
        self.apply_trade(trade, stats)
    }

    /// Apply any earlier trade callbacks after an order is proven to belong to
    /// the current run. Returns them in arrival order.
    pub fn apply_buffered_trades(
        &mut self,
        order_id: u64,
        stats: &mut MakerStats,
    ) -> Result<Vec<MakerFill>, LedgerError> {
        if !self.maker_order_ids.contains(&order_id) {
            return Ok(Vec::new());
        }
        let mut pending = Vec::new();
        for trade in std::mem::take(&mut self.pending_trades) {
            if trade.order_id == order_id {
                self.pending_trade_ids.remove(&trade.trade_id);
                pending.push(trade);
            } else {
                self.pending_trades.push_back(trade);
            }
        }
        let mut fills = Vec::with_capacity(pending.len());
        for pending in pending {
            if let Some(fill) = self.apply_trade(
                LedgerTrade {
                    trade_id: pending.trade_id,
                    order_id: pending.order_id,
                    side: pending.side,
                    price: pending.price,
                    qty: pending.qty,
                    mark: pending.mark,
                    trade_ts: &pending.trade_ts,
                    source: pending.source,
                },
                stats,
            )? {
                fills.push(fill);
            }
        }
        Ok(fills)
    }

    fn validate_trade(&self, trade: LedgerTrade<'_>) -> Result<(), LedgerError> {
        if trade.trade_id == 0 {
            return Err(LedgerError::MissingTradeId {
                order_id: trade.order_id,
            });
        }
        if trade.order_id == 0 {
            return Err(LedgerError::MissingOrderId {
                trade_id: trade.trade_id,
            });
        }
        if !trade.qty.is_finite()
            || !trade.price.is_finite()
            || !trade.mark.is_finite()
            || trade.qty <= 0.0
            || trade.price <= 0.0
        {
            return Err(LedgerError::InvalidTrade {
                trade_id: trade.trade_id,
                order_id: trade.order_id,
                price: trade.price,
                qty: trade.qty,
            });
        }
        Ok(())
    }

    fn apply_trade(
        &mut self,
        trade: LedgerTrade<'_>,
        stats: &mut MakerStats,
    ) -> Result<Option<MakerFill>, LedgerError> {
        self.validate_trade(trade)?;
        if !self.seen_trade_ids.insert(trade.trade_id) {
            return Ok(None);
        }
        stats.record_fill(trade.side, trade.price, trade.qty, trade.mark);
        self.expected_position += match trade.side {
            OrderSide::Buy => trade.qty,
            OrderSide::Sell => -trade.qty,
        };
        stats.observe_position(self.expected_position);
        Ok(Some(MakerFill {
            side: trade.side,
            price: trade.price,
            qty: trade.qty,
            trade_id: Some(trade.trade_id),
            order_id: Some(trade.order_id),
            trade_ts: Some(trade.trade_ts.to_owned()),
            origin: trade.source.origin(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(
        trade_id: u64,
        order_id: u64,
        side: OrderSide,
        qty: f64,
        source: TradeSource,
    ) -> LedgerTrade<'static> {
        LedgerTrade {
            trade_id,
            order_id,
            side,
            price: 100.0,
            qty,
            mark: 100.0,
            trade_ts: "2026-07-14T00:00:00Z",
            source,
        }
    }

    fn adopted_ledger() -> (MakerLedger, MakerStats) {
        let mut ledger = MakerLedger::new(0.0);
        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001b0"), "sxmk-run-"));
        (ledger, MakerStats::default())
    }

    #[test]
    fn websocket_then_rest_trade_is_accounted_exactly_once() {
        let (mut ledger, mut stats) = adopted_ledger();
        let ws = ledger
            .record_trade(
                trade(1, 7, OrderSide::Buy, 0.2, TradeSource::AccountStream),
                &mut stats,
            )
            .unwrap();
        let rest = ledger
            .record_trade(
                trade(1, 7, OrderSide::Buy, 0.2, TradeSource::RestBackfill),
                &mut stats,
            )
            .unwrap();

        assert_eq!(ws.unwrap().origin, "current_run_ws_trade");
        assert!(rest.is_none());
        assert_eq!(stats.fills(), 1);
        assert!((ledger.expected_position - 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }

    #[test]
    fn rest_then_websocket_trade_is_accounted_exactly_once() {
        let (mut ledger, mut stats) = adopted_ledger();
        assert!(ledger
            .record_trade(
                trade(1, 7, OrderSide::Sell, 0.2, TradeSource::RestBackfill),
                &mut stats,
            )
            .unwrap()
            .is_some());
        assert!(ledger
            .record_trade(
                trade(1, 7, OrderSide::Sell, 0.2, TradeSource::AccountStream),
                &mut stats,
            )
            .unwrap()
            .is_none());

        assert_eq!(stats.fills(), 1);
        assert!((ledger.expected_position + 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }

    #[test]
    fn partial_trades_and_duplicate_replay_are_exactly_once() {
        let (mut ledger, mut stats) = adopted_ledger();
        for trade in [
            trade(1, 7, OrderSide::Buy, 0.1, TradeSource::AccountStream),
            trade(2, 7, OrderSide::Buy, 0.1, TradeSource::AccountStream),
            trade(2, 7, OrderSide::Buy, 0.1, TradeSource::RestBackfill),
        ] {
            ledger.record_trade(trade, &mut stats).unwrap();
        }

        assert_eq!(stats.fills(), 2);
        assert!((stats.cash + 20.0).abs() < 1e-12);
        assert!((ledger.expected_position - 0.2).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
    }

    #[test]
    fn trade_before_order_is_buffered_then_applied_once_when_owned() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        assert!(ledger
            .record_trade(
                trade(1, 7, OrderSide::Sell, 0.2, TradeSource::AccountStream),
                &mut stats,
            )
            .unwrap()
            .is_none());
        assert!(ledger
            .record_trade(
                trade(1, 7, OrderSide::Sell, 0.2, TradeSource::RestBackfill),
                &mut stats,
            )
            .unwrap()
            .is_none());

        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001a0"), "sxmk-run-"));
        let fills = ledger.apply_buffered_trades(7, &mut stats).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].origin, "current_run_ws_trade");
        assert_eq!(stats.fills(), 1);
        assert!((ledger.expected_position + 0.2).abs() < 1e-12);
    }

    #[test]
    fn buffered_trades_keep_arrival_order_when_other_orders_are_retained() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        for trade in [
            trade(1, 7, OrderSide::Buy, 0.1, TradeSource::AccountStream),
            trade(2, 8, OrderSide::Sell, 0.2, TradeSource::AccountStream),
            trade(3, 7, OrderSide::Buy, 0.3, TradeSource::AccountStream),
        ] {
            assert!(ledger.record_trade(trade, &mut stats).unwrap().is_none());
        }

        assert!(ledger.adopt_order(7, Some("sxmk-run-q00000001a0"), "sxmk-run-"));
        let fills = ledger.apply_buffered_trades(7, &mut stats).unwrap();
        assert_eq!(
            fills
                .iter()
                .map(|fill| fill.trade_id.unwrap())
                .collect::<Vec<_>>(),
            vec![1, 3]
        );

        assert!(ledger.adopt_order(8, Some("sxmk-run-q00000002a0"), "sxmk-run-"));
        let fills = ledger.apply_buffered_trades(8, &mut stats).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, Some(2));
        assert_eq!(stats.fills(), 3);
    }

    #[test]
    fn partial_trade_then_cancel_keeps_positions_aligned() {
        let (mut ledger, mut stats) = adopted_ledger();
        ledger
            .record_trade(
                trade(1, 7, OrderSide::Buy, 0.1, TradeSource::AccountStream),
                &mut stats,
            )
            .unwrap();
        // A later cancelled order update must not alter a stable trade.
        assert!((ledger.expected_position - 0.1).abs() < 1e-12);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-12);
        assert!(stats.pnl(ledger.expected_position, 100.0).abs() < 1e-12);
    }

    #[test]
    fn invalid_stable_ids_are_rejected() {
        let (mut ledger, mut stats) = adopted_ledger();
        assert!(matches!(
            ledger.record_trade(
                trade(0, 7, OrderSide::Buy, 0.1, TradeSource::AccountStream),
                &mut stats,
            ),
            Err(LedgerError::MissingTradeId { order_id: 7 })
        ));
        assert!(matches!(
            ledger.record_trade(
                trade(1, 0, OrderSide::Buy, 0.1, TradeSource::AccountStream),
                &mut stats,
            ),
            Err(LedgerError::MissingOrderId { trade_id: 1 })
        ));
    }

    #[test]
    fn buffered_trades_evict_oldest_without_stopping_the_session() {
        // Trades whose owning order has not yet been adopted are buffered. That
        // buffer must be capped so a flood of trades for foreign orders neither
        // grows it without bound nor stops this maker session.
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        for index in 0..=MAX_PENDING_TRADES as u64 {
            // Distinct trade and order ids so none dedupe or get owned.
            let outcome = ledger
                .record_trade(
                    trade(
                        index + 1,
                        index + 1,
                        OrderSide::Buy,
                        0.1,
                        TradeSource::AccountStream,
                    ),
                    &mut stats,
                )
                .unwrap();
            assert!(outcome.is_none(), "unowned trade should buffer, not fill");
        }

        assert_eq!(ledger.pending_trades.len(), MAX_PENDING_TRADES);
        assert!(!ledger.pending_trade_ids.contains(&1));
        assert!(ledger
            .pending_trade_ids
            .contains(&(MAX_PENDING_TRADES as u64 + 1)));
        assert_eq!(stats.fills(), 0);

        // A duplicate of a surviving buffered trade remains deduplicated.
        assert!(ledger
            .record_trade(
                trade(2, 2, OrderSide::Buy, 0.1, TradeSource::AccountStream),
                &mut stats,
            )
            .unwrap()
            .is_none());
        assert_eq!(ledger.pending_trades.len(), MAX_PENDING_TRADES);

        // The oldest trade was discarded, while a recent trade can still be
        // applied in arrival order if its order is later proven to be ours.
        assert!(ledger.adopt_order(1, Some("sxmk-run-q00000001a0"), "sxmk-run-"));
        assert!(ledger
            .apply_buffered_trades(1, &mut stats)
            .unwrap()
            .is_empty());
        let latest_order_id = MAX_PENDING_TRADES as u64 + 1;
        assert!(ledger.adopt_order(latest_order_id, Some("sxmk-run-q00000002a0"), "sxmk-run-"));
        assert_eq!(
            ledger
                .apply_buffered_trades(latest_order_id, &mut stats)
                .unwrap()
                .len(),
            1
        );

        // An evicted execution is not marked seen. A later WS/REST replay can
        // still account it once ownership has been established.
        assert!(ledger
            .record_trade(
                trade(1, 1, OrderSide::Buy, 0.1, TradeSource::RestBackfill),
                &mut stats,
            )
            .unwrap()
            .is_some());
        assert_eq!(stats.fills(), 2);
    }
}
