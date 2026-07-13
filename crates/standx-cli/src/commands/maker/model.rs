use standx_sdk::error::Error as StandxError;
use standx_sdk::models::{Order, OrderSide, Position};

pub(super) enum MakerExit {
    CtrlC,
    OrderResponse(String),
    ConsecutiveErrors(String),
    PositionReconciliation(String),
}

impl MakerExit {
    pub(super) fn lifecycle_reason(&self) -> String {
        match self {
            Self::CtrlC => "Ctrl+C".to_string(),
            Self::OrderResponse(error) => {
                format!("fail-safe: order-response stream unavailable: {error}")
            }
            Self::ConsecutiveErrors(error) => {
                format!("fail-safe: 3 consecutive maker cycle errors: {error}")
            }
            Self::PositionReconciliation(error) => {
                format!("fail-safe: position reconciliation failed: {error}")
            }
        }
    }

    pub(super) fn terminal_error(&self) -> Option<String> {
        match self {
            Self::CtrlC => None,
            Self::OrderResponse(error) => Some(format!(
                "maker stopped immediately (fail-safe): order-response stream unavailable: {error}"
            )),
            Self::ConsecutiveErrors(error) => Some(format!(
                "maker stopped after 3 consecutive maker cycle errors (fail-safe): {error}"
            )),
            Self::PositionReconciliation(error) => Some(format!(
                "maker stopped immediately (fail-safe): position reconciliation failed: {error}"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct MakerFill {
    pub(super) side: OrderSide,
    pub(super) price: f64,
    pub(super) qty: f64,
    pub(super) trade_id: Option<u64>,
    pub(super) order_id: Option<u64>,
    pub(super) trade_ts: Option<String>,
    pub(super) origin: &'static str,
}

pub(super) struct PendingPlace {
    pub(super) request_id: String,
    pub(super) cl_ord_id: String,
    pub(super) side: OrderSide,
    pub(super) price: f64,
    pub(super) qty: f64,
    pub(super) level: u32,
    pub(super) ref_center: f64,
    pub(super) cycle: u64,
}

pub(super) fn is_maker_order(order: &Order) -> bool {
    standx_maker::is_maker_client_order_id(order.cl_ord_id.as_deref())
}

pub(super) fn is_current_run_order(order: &Order, run_order_prefix: &str) -> bool {
    standx_maker::is_current_run_client_order_id(order.cl_ord_id.as_deref(), run_order_prefix)
}

pub(super) fn position_for_symbol(positions: &[Position], symbol: &str) -> anyhow::Result<f64> {
    positions
        .iter()
        .filter(|position| position.symbol.eq_ignore_ascii_case(symbol))
        .try_fold(0.0, |total, position| {
            let qty = position.qty.parse::<f64>().map_err(|_| {
                anyhow::anyhow!("position on {symbol} has invalid qty '{}'", position.qty)
            })?;
            if !qty.is_finite() {
                return Err(anyhow::anyhow!(
                    "position on {symbol} has invalid non-finite qty"
                ));
            }
            let signed_qty = match position.side {
                Some(OrderSide::Sell) => -qty.abs(),
                Some(OrderSide::Buy) => qty.abs(),
                None => qty,
            };
            Ok(total + signed_qty)
        })
}

pub(super) fn is_order_rejection(error: &StandxError) -> bool {
    matches!(
        error,
        StandxError::Api {
            retryable: false,
            ..
        }
    )
}
