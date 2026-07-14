//! SDK payload adapter for the pure maker ledger.

use anyhow::Result;
use standx_maker::{LedgerTrade, MakerFill, MakerLedger, MakerStats, TradeSource};
use standx_sdk::account_stream::{OrderUpdate, TradeUpdate};
use standx_sdk::models::{Order, OrderSide, Trade};

pub(super) fn adopt_order(
    ledger: &mut MakerLedger,
    order: &Order,
    run_order_prefix: &str,
) -> Result<bool> {
    let client_order_id = order.cl_ord_id.as_deref();
    if !standx_maker::is_current_run_client_order_id(client_order_id, run_order_prefix) {
        return Ok(false);
    }
    let order_id = order.id.parse::<u64>().map_err(|_| {
        anyhow::anyhow!(
            "current-run maker order has non-integer exchange ID '{}'",
            order.id
        )
    })?;
    Ok(ledger.adopt_order(order_id, client_order_id, run_order_prefix))
}

pub(super) fn apply_order_update(
    ledger: &mut MakerLedger,
    update: &OrderUpdate,
    symbol: &str,
    run_order_prefix: &str,
    mark: f64,
    stats: &mut MakerStats,
    fills: &mut Vec<MakerFill>,
) -> Result<bool> {
    if update.symbol != symbol {
        return Ok(false);
    }
    if !ledger.adopt_order(
        update.order_id,
        update.cl_ord_id.as_deref(),
        run_order_prefix,
    ) {
        return Ok(false);
    }
    let exit = ledger.is_exit_order(update.order_id);
    let buffered = ledger.apply_buffered_trades(update.order_id, stats)?;
    let saw_exit_fill = exit && !buffered.is_empty();
    fills.extend(buffered);
    // The cumulative fill fields in an order callback are deliberately not
    // booked here. Only a stable-ID TradeUpdate or REST trade may mutate PnL
    // and expected position.
    let _ = mark;
    Ok(saw_exit_fill)
}

pub(super) fn apply_account_trade(
    ledger: &mut MakerLedger,
    trade: TradeUpdate,
    symbol: &str,
    mark: f64,
    stats: &mut MakerStats,
    fills: &mut Vec<MakerFill>,
) -> Result<bool> {
    if !trade.symbol.eq_ignore_ascii_case(symbol) {
        return Ok(false);
    }
    let (price, qty) = trade_values(trade.trade_id, &trade.price, &trade.qty)?;
    apply_ledger_trade(
        ledger,
        LedgerTrade {
            trade_id: trade.trade_id,
            order_id: trade.order_id,
            side: trade.side,
            price,
            qty,
            mark,
            trade_ts: &trade.trade_ts,
            source: TradeSource::AccountStream,
        },
        stats,
        fills,
    )
}

pub(super) fn apply_rest_trade(
    ledger: &mut MakerLedger,
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
    let (side, price, qty) = maker_trade_fill(&trade)?;
    apply_ledger_trade(
        ledger,
        LedgerTrade {
            trade_id: trade.id,
            order_id,
            side,
            price,
            qty,
            mark,
            trade_ts: &trade.time,
            source: TradeSource::RestBackfill,
        },
        stats,
        fills,
    )
}

fn apply_ledger_trade(
    ledger: &mut MakerLedger,
    trade: LedgerTrade<'_>,
    stats: &mut MakerStats,
    fills: &mut Vec<MakerFill>,
) -> Result<bool> {
    let exit = ledger.is_exit_order(trade.order_id);
    if let Some(fill) = ledger.record_trade(trade, stats)? {
        fills.push(fill);
        return Ok(exit);
    }
    Ok(false)
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
    let (price, qty) = trade_values(trade.id, &trade.price, &trade.qty)?;
    Ok((side, price, qty))
}

fn trade_values(trade_id: u64, price: &str, qty: &str) -> Result<(f64, f64)> {
    let price = price
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("maker trade {trade_id} has invalid price '{price}'"))?;
    let qty = qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("maker trade {trade_id} has invalid qty '{qty}'"))?;
    if !price.is_finite() || price <= 0.0 || !qty.is_finite() || qty <= 0.0 {
        return Err(anyhow::anyhow!(
            "maker trade {trade_id} has non-positive price/qty"
        ));
    }
    Ok((price, qty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use standx_sdk::models::OrderStatus;

    fn order_update(side: OrderSide, fill_qty: &str) -> OrderUpdate {
        OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-run-q00000001a0".to_string()),
            symbol: "BTC-USD".to_string(),
            side,
            qty: "0.20".to_string(),
            fill_qty: fill_qty.to_string(),
            fill_avg_price: "100.00".to_string(),
            price: "100.00".to_string(),
            status: OrderStatus::Filled,
            reduce_only: false,
            updated_at: "2026-07-13T00:00:00Z".to_string(),
        }
    }

    fn trade_update(side: OrderSide, price: &str, qty: &str) -> TradeUpdate {
        TradeUpdate {
            seq: 2,
            trade_id: 11,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side,
            price: price.to_string(),
            qty: qty.to_string(),
            trade_ts: "2026-07-13T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn typed_account_trade_is_the_only_order_callback_accounting_path() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();

        apply_order_update(
            &mut ledger,
            &order_update(OrderSide::Sell, "-0.20"),
            "BTC-USD",
            "sxmk-run-",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        assert!(
            fills.is_empty(),
            "cumulative order fills must not be booked"
        );
        apply_account_trade(
            &mut ledger,
            trade_update(OrderSide::Sell, "100.00", "0.20"),
            "BTC-USD",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].side, OrderSide::Sell);
        assert!((fills[0].qty - 0.20).abs() < 1e-9);
        assert!((ledger.expected_position + 0.20).abs() < 1e-9);
    }

    #[test]
    fn non_finite_typed_trade_quantity_is_rejected_explicitly() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();

        apply_order_update(
            &mut ledger,
            &order_update(OrderSide::Sell, "NaN"),
            "BTC-USD",
            "sxmk-run-",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        let error = apply_account_trade(
            &mut ledger,
            trade_update(OrderSide::Sell, "100.00", "NaN"),
            "BTC-USD",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap_err();

        assert!(error.to_string().contains("non-positive price/qty"));
    }

    #[test]
    fn partial_fill_then_cancelled_keeps_ledger_and_stats_positions_aligned() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        let mut update = order_update(OrderSide::Buy, "0.10");
        update.status = OrderStatus::Canceled;

        apply_order_update(
            &mut ledger,
            &update,
            "BTC-USD",
            "sxmk-run-",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        apply_account_trade(
            &mut ledger,
            trade_update(OrderSide::Buy, "100.00", "0.10"),
            "BTC-USD",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        assert_eq!(fills.len(), 1);
        assert!((ledger.expected_position - 0.10).abs() < 1e-9);
        assert!((stats.position() - ledger.expected_position).abs() < 1e-9);
        assert!(stats.pnl(ledger.expected_position, 100.0).abs() < 1e-9);
    }
}
