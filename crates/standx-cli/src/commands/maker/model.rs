use standx_sdk::error::Error as StandxError;
use standx_sdk::models::{Order, OrderSide, Position};

/// Process exit code emitted when the maker performs an *intentional*
/// fail-safe shutdown: the order-response stream was lost, three maker
/// cycles failed in a row, position reconciliation failed, or residual
/// maker-owned orders could not be cancelled on the way out.
///
/// Supervisors must treat this as "stop, do NOT auto-restart, notify a
/// human" (systemd `RestartPreventExitStatus=`). It is deliberately
/// distinct from `0` (a clean Ctrl+C / SIGTERM stop: no restart, no alert),
/// from `1` (a generic startup/config/validation error), and from a panic
/// (`101`) or a fatal signal (e.g. SIGKILL -> `137`), so that an
/// *unexpected* death remains restartable while a designed fail-safe exit
/// does not trigger a restart loop.
pub const FAIL_SAFE_EXIT_CODE: i32 = 75;

/// Typed marker for an intentional maker fail-safe shutdown. Carrying the
/// reason as a concrete error (rather than a bare `anyhow::anyhow!`) lets
/// `main` downcast it and map it to [`FAIL_SAFE_EXIT_CODE`] while still
/// printing the message through the normal error path.
#[derive(Debug)]
pub struct FailSafeShutdown {
    pub message: String,
}

impl std::fmt::Display for FailSafeShutdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FailSafeShutdown {}

pub(super) enum MakerExit {
    CtrlC,
    OrderResponse(String),
    ConsecutiveErrors(String),
    PositionReconciliation(String),
    StopLoss(String),
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
            Self::StopLoss(detail) => {
                format!("fail-safe: stop-loss breached: {detail}")
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
            Self::StopLoss(detail) => Some(format!(
                "maker stopped immediately (fail-safe): stop-loss breached: {detail}"
            )),
        }
    }
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
            let signed_qty =
                signed_position_quantity(&position.qty, position.side).map_err(|error| {
                    anyhow::anyhow!("position on {symbol} has invalid qty: {error}")
                })?;
            Ok(total + signed_qty)
        })
}

pub(super) fn signed_position_quantity(
    raw_qty: &str,
    side: Option<OrderSide>,
) -> anyhow::Result<f64> {
    let qty = raw_qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("'{raw_qty}' is not numeric"))?;
    if !qty.is_finite() {
        return Err(anyhow::anyhow!("'{raw_qty}' is not finite"));
    }
    Ok(match side {
        Some(OrderSide::Sell) => -qty.abs(),
        Some(OrderSide::Buy) => qty.abs(),
        None => qty,
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
