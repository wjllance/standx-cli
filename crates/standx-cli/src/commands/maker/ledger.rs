//! SDK payload adapter for the pure maker ledger.

use anyhow::Result;
use standx_maker::{CumulativeFill, MakerFill, MakerLedger, MakerStats, RestFill};
use standx_sdk::account_stream::OrderUpdate;
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
    let qty = update
        .fill_qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("account order {} has invalid fill_qty", update.order_id))?;
    if !qty.is_finite() {
        return Err(anyhow::anyhow!(
            "account order {} has non-finite fill_qty",
            update.order_id
        ));
    }
    let qty = qty.abs();
    if qty == 0.0 {
        return Ok(false);
    }
    let average = update.fill_avg_price.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "account order {} has invalid fill_avg_price",
            update.order_id
        )
    })?;
    let exit = ledger.is_exit_order(update.order_id);
    if let Some(fill) = ledger.record_cumulative_fill(
        CumulativeFill {
            order_id: update.order_id,
            side: update.side,
            qty,
            notional: qty * average,
            mark,
            origin: "current_run_ws_order",
            trade_id: None,
            trade_ts: Some(&update.updated_at),
        },
        stats,
    )? {
        fills.push(fill);
        return Ok(exit);
    }
    Ok(false)
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
    if !ledger.maker_order_ids.contains(&order_id) {
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
    let (side, price, qty) = maker_trade_fill(&trade)?;
    let exit = ledger.is_exit_order(order_id);
    if let Some(fill) = ledger.record_rest_fill(
        RestFill {
            trade_id: trade.id,
            order_id,
            side,
            price,
            qty,
            mark,
            trade_ts: &trade.time,
        },
        stats,
    )? {
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

    #[test]
    fn signed_sell_fill_quantity_is_normalized_before_ledger_ingestion() {
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

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].side, OrderSide::Sell);
        assert!((fills[0].qty - 0.20).abs() < 1e-9);
        assert!((ledger.expected_position + 0.20).abs() < 1e-9);
    }

    #[test]
    fn non_finite_ws_fill_quantity_is_rejected_explicitly() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();

        let error = apply_order_update(
            &mut ledger,
            &order_update(OrderSide::Sell, "NaN"),
            "BTC-USD",
            "sxmk-run-",
            100.0,
            &mut stats,
            &mut fills,
        )
        .unwrap_err();

        assert!(error.to_string().contains("non-finite fill_qty"));
    }
}
