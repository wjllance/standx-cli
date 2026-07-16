use super::output::{
    emit_live_fill, emit_reconciliation_snapshot_error, emit_reconciliation_state,
    emit_stop_loss_triggered,
};
use super::*;
use standx_sdk::order_response::OrderResponse;

mod cycle_flow;
mod events;
mod lifecycle;
mod recovery_flow;
mod state;

#[cfg(test)]
use cycle_flow::{commit_cycle_effect, take_cycle_work};
#[cfg(test)]
pub(super) use events::apply_order_responses;
use events::{
    absorb_account_outcome, account_event_invalidates_cycle, accounting_position_mismatch,
    apply_account_event, apply_account_events, apply_order_response,
    apply_order_responses_observed, duration_ms, invalidate_session_latency,
    market_update_requires_replan, observe_order_ack, order_request_timeout_detail,
    order_response_failure, reconciliation_error_for_cycle, request_timeout_notice,
    schedule_account_balance_refresh, AccountEventContext, AccountEventState,
    OrderResponseObservation, OutcomeSink, ORDER_REQUEST_TIMEOUT,
};
#[cfg(test)]
use events::{order_response_correlation_failed, AccountEventOutcome, CancelRejection};
use recovery_flow::*;
use state::*;
pub(super) async fn run_maker(
    symbol: String,
    args: MakerRunArgs,
    output_format: OutputFormat,
) -> Result<()> {
    let startup = run_startup(symbol, &args, output_format).await?;
    MakerRuntime::announce_start(&args, output_format, &startup).await;
    let runtime = MakerRuntime::new(args, output_format, startup)?;
    let (runtime, exit) = runtime.drive().await;
    runtime.shutdown(exit).await
}

#[cfg(test)]
mod tests;
