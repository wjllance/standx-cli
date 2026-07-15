use standx_sdk::account_stream::OrderUpdate;
use standx_sdk::models::{Order, OrderSide, OrderStatus, Position};

/// Process exit code emitted when the maker performs an *intentional*
/// fail-safe shutdown: the order-response stream was lost, three maker
/// cycles failed in a row, position reconciliation or an internal accounting
/// invariant failed, or residual maker-owned orders could not be cancelled on
/// the way out.
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

#[derive(Debug)]
pub(super) enum MakerExit {
    CtrlC,
    OrderResponse(String),
    ConsecutiveErrors(String),
    PositionReconciliation(String),
    AccountingInvariant(String),
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
            Self::AccountingInvariant(detail) => {
                format!("fail-safe: accounting invariant failed: {detail}")
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
            Self::AccountingInvariant(detail) => Some(format!(
                "maker stopped immediately (fail-safe): accounting invariant failed: {detail}"
            )),
            Self::StopLoss(detail) => Some(format!(
                "maker stopped immediately (fail-safe): stop-loss breached: {detail}"
            )),
        }
    }
}

impl From<standx_maker::RuntimeStopReason> for MakerExit {
    fn from(reason: standx_maker::RuntimeStopReason) -> Self {
        match reason {
            standx_maker::RuntimeStopReason::CtrlC => Self::CtrlC,
            standx_maker::RuntimeStopReason::OrderResponse(detail) => Self::OrderResponse(detail),
            standx_maker::RuntimeStopReason::PositionReconciliation(detail) => {
                Self::PositionReconciliation(detail)
            }
            standx_maker::RuntimeStopReason::CleanupFailure { target, reason } => match target {
                standx_maker::RecoveryTarget::OrderResponse => Self::OrderResponse(reason),
                standx_maker::RecoveryTarget::AccountStream
                | standx_maker::RecoveryTarget::PositionReconciliation => {
                    Self::PositionReconciliation(reason)
                }
            },
            standx_maker::RuntimeStopReason::ConsecutiveCycleErrors(detail) => {
                Self::ConsecutiveErrors(detail)
            }
            standx_maker::RuntimeStopReason::StopLoss(detail) => Self::StopLoss(detail),
        }
    }
}

pub(super) fn is_maker_order(order: &Order) -> bool {
    standx_maker::is_maker_client_order_id(order.cl_ord_id.as_deref())
}

fn terminal_order_status(status: OrderStatus) -> bool {
    matches!(
        status,
        OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Rejected | OrderStatus::Expired
    )
}

pub(super) fn rest_order_observation(
    order: &Order,
) -> anyhow::Result<standx_maker::OrderObservation> {
    let order_id = order
        .id
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("order has non-integer exchange ID '{}'", order.id))?;
    let price = order
        .price
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("order {order_id} has invalid price '{}'", order.price))?;
    let open_qty = order
        .qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("order {order_id} has invalid qty '{}'", order.qty))?;
    if !price.is_finite() || !open_qty.is_finite() || price <= 0.0 || open_qty < 0.0 {
        return Err(anyhow::anyhow!(
            "order {order_id} has invalid projection values price={price}, qty={open_qty}"
        ));
    }
    Ok(standx_maker::OrderObservation {
        order_id,
        client_order_id: order.cl_ord_id.clone(),
        side: order.side,
        price,
        open_qty,
        terminal: terminal_order_status(order.status),
    })
}

pub(super) fn stream_order_observation(
    order: &OrderUpdate,
) -> anyhow::Result<standx_maker::OrderObservation> {
    let terminal = terminal_order_status(order.status);
    let raw_price = if order.price.is_empty() || order.price == "0" {
        &order.fill_avg_price
    } else {
        &order.price
    };
    let price = if raw_price.is_empty() {
        0.0
    } else {
        raw_price.parse::<f64>().map_err(|_| {
            anyhow::anyhow!(
                "account order {} has invalid price '{}'",
                order.order_id,
                raw_price
            )
        })?
    };
    let qty = order.qty.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "account order {} has invalid qty '{}'",
            order.order_id,
            order.qty
        )
    })?;
    let fill_qty = order.fill_qty.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "account order {} has invalid fill qty '{}'",
            order.order_id,
            order.fill_qty
        )
    })?;
    let open_qty = (qty - fill_qty).max(0.0);
    if !price.is_finite()
        || !qty.is_finite()
        || !fill_qty.is_finite()
        || (!terminal && price <= 0.0)
        || qty < 0.0
        || fill_qty < 0.0
    {
        return Err(anyhow::anyhow!(
            "account order {} has invalid projection values",
            order.order_id
        ));
    }
    Ok(standx_maker::OrderObservation {
        order_id: order.order_id,
        client_order_id: order.cl_ord_id.clone(),
        side: order.side,
        price,
        open_qty,
        terminal,
    })
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

#[cfg(test)]
mod exit_mapping_tests {
    use super::MakerExit;
    use standx_maker::{RecoveryTarget, RuntimeStopReason};

    /// Pins the full RuntimeStopReason → MakerExit mapping so a new stop
    /// reason (or a re-targeted CleanupFailure) cannot silently land in the
    /// wrong fail-safe exit bucket.
    #[test]
    fn every_runtime_stop_reason_maps_to_the_expected_exit() {
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::CtrlC),
            MakerExit::CtrlC
        ));
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::OrderResponse("boom".to_string())),
            MakerExit::OrderResponse(detail) if detail == "boom"
        ));
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::PositionReconciliation("boom".to_string())),
            MakerExit::PositionReconciliation(detail) if detail == "boom"
        ));
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::ConsecutiveCycleErrors("boom".to_string())),
            MakerExit::ConsecutiveErrors(detail) if detail == "boom"
        ));
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::StopLoss("boom".to_string())),
            MakerExit::StopLoss(detail) if detail == "boom"
        ));
        assert!(matches!(
            MakerExit::from(RuntimeStopReason::CleanupFailure {
                target: RecoveryTarget::OrderResponse,
                reason: "boom".to_string(),
            }),
            MakerExit::OrderResponse(detail) if detail == "boom"
        ));
        for target in [
            RecoveryTarget::AccountStream,
            RecoveryTarget::PositionReconciliation,
        ] {
            assert!(matches!(
                MakerExit::from(RuntimeStopReason::CleanupFailure {
                    target,
                    reason: "boom".to_string(),
                }),
                MakerExit::PositionReconciliation(detail) if detail == "boom"
            ));
        }
    }

    /// Every fail-safe exit must surface a terminal error (only a clean
    /// Ctrl+C stop is silent) so supervisors always see a reason on exit 75.
    #[test]
    fn only_ctrl_c_exits_without_a_terminal_error() {
        assert!(MakerExit::CtrlC.terminal_error().is_none());
        for exit in [
            MakerExit::OrderResponse("boom".to_string()),
            MakerExit::ConsecutiveErrors("boom".to_string()),
            MakerExit::PositionReconciliation("boom".to_string()),
            MakerExit::AccountingInvariant("boom".to_string()),
            MakerExit::StopLoss("boom".to_string()),
        ] {
            let error = exit
                .terminal_error()
                .expect("fail-safe exits carry an error");
            assert!(error.contains("boom"));
            assert!(exit.lifecycle_reason().contains("boom"));
        }
    }
}
