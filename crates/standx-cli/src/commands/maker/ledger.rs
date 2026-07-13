use super::model::{is_current_run_order, MakerFill};
use anyhow::Result;
use standx_maker::MakerStats;
use standx_sdk::account_stream::OrderUpdate;
use standx_sdk::models::{Order, OrderSide, Trade};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default)]
struct FillTotals {
    qty: f64,
    notional: f64,
}

#[derive(Debug)]
pub(super) struct MakerLedger {
    pub(super) expected_position: f64,
    pub(super) maker_order_ids: HashSet<u64>,
    pub(super) exit_order_ids: HashSet<u64>,
    seen_fill_ids: HashSet<u64>,
    accounted: HashMap<u64, FillTotals>,
    rest_seen: HashMap<u64, FillTotals>,
}

impl MakerLedger {
    pub(super) fn new(starting_position: f64) -> Self {
        Self {
            expected_position: starting_position,
            maker_order_ids: HashSet::new(),
            exit_order_ids: HashSet::new(),
            seen_fill_ids: HashSet::new(),
            accounted: HashMap::new(),
            rest_seen: HashMap::new(),
        }
    }

    pub(super) fn adopt_order(&mut self, order: &Order, run_order_prefix: &str) -> Result<bool> {
        if !is_current_run_order(order, run_order_prefix) {
            return Ok(false);
        }
        let order_id = order.id.parse::<u64>().map_err(|_| {
            anyhow::anyhow!(
                "current-run maker order has non-integer exchange ID '{}'",
                order.id
            )
        })?;
        self.maker_order_ids.insert(order_id);
        if order
            .cl_ord_id
            .as_deref()
            .is_some_and(|id| id.starts_with(&format!("{run_order_prefix}x")))
        {
            self.exit_order_ids.insert(order_id);
        }
        Ok(true)
    }

    fn record_delta(
        &mut self,
        fill: LedgerFill,
        stats: &mut MakerStats,
    ) -> Result<Option<MakerFill>> {
        let previous = self
            .accounted
            .get(&fill.order_id)
            .copied()
            .unwrap_or_default();
        let qty = fill.cumulative.qty - previous.qty;
        if qty <= 1e-12 {
            return Ok(None);
        }
        let notional = fill.cumulative.notional - previous.notional;
        let price = notional / qty;
        if !qty.is_finite() || !price.is_finite() || qty <= 0.0 || price <= 0.0 {
            return Err(anyhow::anyhow!(
                "invalid cumulative fill for maker order {}: qty={qty}, price={price}",
                fill.order_id
            ));
        }
        stats.record_fill(fill.side, price, qty, fill.mark);
        self.expected_position += match fill.side {
            OrderSide::Buy => qty,
            OrderSide::Sell => -qty,
        };
        self.accounted.insert(fill.order_id, fill.cumulative);
        Ok(Some(MakerFill {
            side: fill.side,
            price,
            qty,
            trade_id: fill.trade_id,
            order_id: Some(fill.order_id),
            trade_ts: fill.trade_ts,
            origin: fill.origin,
        }))
    }

    pub(super) fn apply_order_update(
        &mut self,
        update: &OrderUpdate,
        symbol: &str,
        run_order_prefix: &str,
        mark: f64,
        stats: &mut MakerStats,
        fills: &mut Vec<MakerFill>,
    ) -> Result<bool> {
        if update.symbol != symbol
            || !update
                .cl_ord_id
                .as_deref()
                .is_some_and(|id| id.starts_with(run_order_prefix))
        {
            return Ok(false);
        }
        self.maker_order_ids.insert(update.order_id);
        if update
            .cl_ord_id
            .as_deref()
            .is_some_and(|id| id.starts_with(&format!("{run_order_prefix}x")))
        {
            self.exit_order_ids.insert(update.order_id);
        }
        let qty = update.fill_qty.parse::<f64>().map_err(|_| {
            anyhow::anyhow!("account order {} has invalid fill_qty", update.order_id)
        })?;
        if qty <= 0.0 {
            return Ok(false);
        }
        let avg = update.fill_avg_price.parse::<f64>().map_err(|_| {
            anyhow::anyhow!(
                "account order {} has invalid fill_avg_price",
                update.order_id
            )
        })?;
        if let Some(fill) = self.record_delta(
            LedgerFill {
                order_id: update.order_id,
                side: update.side,
                cumulative: FillTotals {
                    qty,
                    notional: qty * avg,
                },
                mark,
                origin: "current_run_ws_order",
                trade_id: None,
                trade_ts: Some(update.updated_at.clone()),
            },
            stats,
        )? {
            fills.push(fill);
            return Ok(self.exit_order_ids.contains(&update.order_id));
        }
        Ok(false)
    }

    pub(super) fn apply_rest_trade(
        &mut self,
        trade: Trade,
        session_started_at: i64,
        now: i64,
        mark: f64,
        stats: &mut MakerStats,
        fills: &mut Vec<MakerFill>,
    ) -> Result<bool> {
        let Some(order_id) = trade.order_id else {
            return Ok(false);
        };
        if !self.maker_order_ids.contains(&order_id) {
            return Ok(false);
        }
        if trade.id == 0 {
            return Err(anyhow::anyhow!(
                "maker fill for order {} has no stable trade ID",
                order_id
            ));
        }
        if !trade_is_in_session(&trade, session_started_at, now)? {
            return Err(anyhow::anyhow!(
                "current-run maker trade {} falls outside the session time boundary",
                trade.id
            ));
        }
        if !self.seen_fill_ids.insert(trade.id) {
            return Ok(false);
        }
        let (side, price, qty) = maker_trade_fill(&trade)?;
        let cumulative = {
            let totals = self.rest_seen.entry(order_id).or_default();
            totals.qty += qty;
            totals.notional += qty * price;
            *totals
        };
        let exit = self.exit_order_ids.contains(&order_id);
        if let Some(fill) = self.record_delta(
            LedgerFill {
                order_id,
                side,
                cumulative,
                mark,
                origin: "current_run_rest_trade",
                trade_id: Some(trade.id),
                trade_ts: Some(trade.time),
            },
            stats,
        )? {
            fills.push(fill);
            Ok(exit)
        } else {
            Ok(false)
        }
    }
}

struct LedgerFill {
    order_id: u64,
    side: OrderSide,
    cumulative: FillTotals,
    mark: f64,
    origin: &'static str,
    trade_id: Option<u64>,
    trade_ts: Option<String>,
}

fn trade_is_in_session(trade: &Trade, session_started_at: i64, now: i64) -> Result<bool> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(&trade.time).map_err(|_| {
        anyhow::anyhow!(
            "maker trade {} has invalid RFC3339 timestamp '{}'",
            trade.id,
            trade.time
        )
    })?;
    let timestamp = timestamp.timestamp();
    Ok(timestamp >= session_started_at && timestamp <= now)
}

pub(super) fn maker_trade_fill(trade: &Trade) -> Result<(OrderSide, f64, f64)> {
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
