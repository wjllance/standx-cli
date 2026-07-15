use super::output::{
    emit_live_fill, emit_reconciliation_snapshot_error, emit_reconciliation_state,
    emit_stop_loss_triggered,
};
use super::*;
use standx_sdk::order_response::OrderResponse;

const ORDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn order_request_timeout_detail(timeout: &TimedOutOrderRequest) -> String {
    format!(
        "order request lifecycle timed out after {:.3}s: kind={} request_id={} waiting_for={}; refusing further live orders",
        timeout.age.as_secs_f64(),
        timeout.kind.label(),
        timeout.request_id,
        timeout.phase.label(),
    )
}

fn take_cleanup_effect(
    runtime_state: &mut MakerState,
    expected_target: RecoveryTarget,
) -> Result<WorkToken> {
    loop {
        match runtime_state.next_effect() {
            Some(MakerEffect::AbortInFlight(_)) => {}
            Some(MakerEffect::Cleanup { token, target }) if target == expected_target => {
                return Ok(token);
            }
            Some(effect) => {
                return Err(anyhow::anyhow!(
                    "runtime expected {expected_target:?} cleanup, got {effect:?}"
                ));
            }
            None => {
                return Err(anyhow::anyhow!(
                    "runtime did not emit {expected_target:?} cleanup"
                ));
            }
        }
    }
}

fn take_recovery_effect(
    runtime_state: &mut MakerState,
    expected_target: RecoveryTarget,
) -> Result<WorkToken> {
    match runtime_state.next_effect() {
        Some(MakerEffect::Recover { token, target }) if target == expected_target => Ok(token),
        Some(effect) => Err(anyhow::anyhow!(
            "runtime expected {expected_target:?} recovery, got {effect:?}"
        )),
        None => Err(anyhow::anyhow!(
            "runtime did not emit {expected_target:?} recovery"
        )),
    }
}

/// Drains queued `AbortInFlight` effects and returns the `Stop` exit the
/// runtime is expected to have queued. The `fallback` names the `MakerExit`
/// variant to use if the runtime is in an unexpected state — the CLI already
/// knows which fault it is handling, so it supplies the variant rather than
/// re-asserting it at each call site.
fn take_stop_effect(
    runtime_state: &mut MakerState,
    fallback: fn(String) -> MakerExit,
) -> MakerExit {
    loop {
        match runtime_state.next_effect() {
            Some(MakerEffect::AbortInFlight(_)) => {}
            Some(MakerEffect::Stop(reason)) => return reason.into(),
            Some(effect) => {
                return fallback(format!("runtime expected stop effect, got {effect:?}"));
            }
            None => return fallback("runtime did not emit stop effect".to_string()),
        }
    }
}

fn recovery_failed_exit(
    runtime_state: &mut MakerState,
    token: WorkToken,
    reason: String,
) -> MakerExit {
    runtime_state.handle(MakerEvent::RecoveryFailed { token, reason });
    take_stop_effect(runtime_state, MakerExit::PositionReconciliation)
}

fn stop_requested_exit(runtime_state: &mut MakerState, reason: RuntimeStopReason) -> MakerExit {
    runtime_state.handle(MakerEvent::StopRequested(reason));
    take_stop_effect(runtime_state, MakerExit::PositionReconciliation)
}

/// Which projection clear an incident flow performs after freeze cleanup.
#[derive(Clone, Copy)]
enum ProjectionReset {
    /// `clear_orders_preserving_pending_acks` — a current-run ack may still
    /// replay after cleanup (account-stream and reconciliation freezes).
    PreservePendingAcks,
    /// `clear_orders_and_pending` — the placement channel is being torn down,
    /// so pending request acks can never arrive (order-response freeze).
    DropPendingRequests,
}

/// How a missing or mismatched runtime effect maps to the flow's stop reason.
/// (`RuntimeStopReason::CleanupFailure` carries the target, so a plain
/// `fn(String) -> RuntimeStopReason` pointer cannot express it.)
#[derive(Clone, Copy)]
enum EffectFailureStop {
    CleanupFailure,
    OrderResponse,
    PositionReconciliation,
}

fn effect_failure_stop(
    kind: EffectFailureStop,
    target: RecoveryTarget,
    reason: String,
) -> RuntimeStopReason {
    match kind {
        EffectFailureStop::CleanupFailure => RuntimeStopReason::CleanupFailure { target, reason },
        EffectFailureStop::OrderResponse => RuntimeStopReason::OrderResponse(reason),
        EffectFailureStop::PositionReconciliation => {
            RuntimeStopReason::PositionReconciliation(reason)
        }
    }
}

fn reset_projection(projection: &mut MakerAccountProjection, reset: ProjectionReset) {
    match reset {
        ProjectionReset::PreservePendingAcks => projection.clear_orders_preserving_pending_acks(),
        ProjectionReset::DropPendingRequests => projection.clear_orders_and_pending(),
    }
}

/// Payload for the position-reconciliation flow's structured
/// `emit_reconciliation_state` emits around freeze and resume.
struct ReconciliationStateNote {
    cause: &'static str,
    expected: f64,
    observed: f64,
}

/// Borrowed bundle of the `run_maker` locals every incident-recovery block
/// touches. Constructed fresh immediately before each helper call; all
/// borrows end when the helper returns, so flow-specific code in between
/// re-borrows the same locals freely.
struct RecoveryIo<'a> {
    runtime_state: &'a mut MakerState,
    notifier: &'a MakerNotifier,
    client: &'a StandXClient,
    session: Option<&'a mut LiveSession>,
    resting: &'a mut Vec<RestingQuote>,
    inventory_exit_pending: &'a mut bool,
    consecutive_errors: &'a mut u32,
    next_cycle_is_recovery: &'a mut bool,
    symbol: &'a str,
    cycle: u64,
    output_format: OutputFormat,
}

/// Everything that differs between the three incident flows' freeze/cleanup
/// preambles. The `notice` is built entirely by the caller so the risk
/// payloads stay reviewable string-for-string at the call sites.
struct FreezeSpec<'a> {
    target: RecoveryTarget,
    trigger: MakerEvent,
    cleanup_effect_stop: EffectFailureStop,
    recovery_effect_stop: EffectFailureStop,
    /// Prepended to `"freeze cleanup failed: {error}"` in the CleanupFailed
    /// reason so each flow keeps its exact historical wording.
    cleanup_failure_prefix: String,
    cleanup_failed_exit: fn(String) -> MakerExit,
    notice: RiskNotice<'a>,
    frozen_note: Option<ReconciliationStateNote>,
    /// Account-stream flow only: abort the stale stream task before
    /// reporting cleanup complete.
    abort_account_stream_handle: bool,
    projection_reset: ProjectionReset,
}

/// Freeze/cleanup preamble shared by the three incident-recovery flows:
/// report the fault to the runtime, drain the cleanup effect, notify,
/// cancel every maker order, reset the local book state, and drain the
/// recovery effect. `Ok` carries the recovery token for the flow-specific
/// reconnect/convergence work; `Err` carries the exit the caller must
/// `break 'main` with.
async fn freeze_and_cleanup_for_recovery(
    io: &mut RecoveryIo<'_>,
    spec: FreezeSpec<'_>,
) -> std::result::Result<WorkToken, MakerExit> {
    io.runtime_state.handle(spec.trigger);
    let cleanup_token = match take_cleanup_effect(io.runtime_state, spec.target) {
        Ok(token) => token,
        Err(error) => {
            return Err(stop_requested_exit(
                io.runtime_state,
                effect_failure_stop(spec.cleanup_effect_stop, spec.target, error.to_string()),
            ));
        }
    };
    if let Some(note) = &spec.frozen_note {
        emit_reconciliation_state(
            io.output_format,
            io.symbol,
            io.cycle,
            "frozen",
            note.cause,
            note.expected,
            note.observed,
        );
    }
    io.notifier.risk(spec.notice, false).await;
    if let Err(error) =
        cancel_maker_orders_with_retry(io.client, io.symbol, 3, io.output_format).await
    {
        io.runtime_state.handle(MakerEvent::CleanupFailed {
            token: cleanup_token,
            reason: format!(
                "{}freeze cleanup failed: {error}",
                spec.cleanup_failure_prefix
            ),
        });
        return Err(take_stop_effect(io.runtime_state, spec.cleanup_failed_exit));
    }
    io.resting.clear();
    if let Some(session) = io.session.as_deref_mut() {
        invalidate_session_latency(session);
        reset_projection(&mut session.projection, spec.projection_reset);
    }
    *io.inventory_exit_pending = false;
    if spec.abort_account_stream_handle {
        if let Some(session) = io.session.as_deref_mut() {
            session.account_stream_handle.abort();
        }
    }
    io.runtime_state
        .handle(MakerEvent::CleanupCompleted(cleanup_token));
    match take_recovery_effect(io.runtime_state, spec.target) {
        Ok(token) => Ok(token),
        Err(error) => Err(stop_requested_exit(
            io.runtime_state,
            effect_failure_stop(spec.recovery_effect_stop, spec.target, error.to_string()),
        )),
    }
}

/// Everything that differs between the three incident flows' resume tails.
/// As with [`FreezeSpec`], the resolved `notice` is built entirely by the
/// caller.
struct ResumeSpec<'a> {
    recovery_token: WorkToken,
    /// Venue position fed to the projection as authoritative after recovery.
    observed: f64,
    projection_reset: ProjectionReset,
    /// Order-response flow only: the paper book is cleared again because the
    /// placement channel was replaced underneath it.
    clear_resting: bool,
    reset_consecutive_errors: bool,
    recovered_note: Option<ReconciliationStateNote>,
    notice: RiskNotice<'a>,
}

/// Resume tail shared by the three incident-recovery flows: reset the
/// projection to an empty verified book, seed it with the reconciled venue
/// position, report RecoverySucceeded, and send the resolved notice. The
/// caller performs its flow-specific final book verification and session
/// half installation *before* calling this, and `continue`s the main loop
/// afterwards.
async fn resume_quoting_after_recovery(io: &mut RecoveryIo<'_>, spec: ResumeSpec<'_>) {
    if spec.clear_resting {
        io.resting.clear();
    }
    if let Some(session) = io.session.as_deref_mut() {
        reset_projection(&mut session.projection, spec.projection_reset);
        let generation = session.projection.generation();
        session.projection.apply(
            generation,
            AccountProjectionEvent::PositionObserved {
                position: spec.observed,
            },
        );
    }
    if spec.reset_consecutive_errors {
        *io.consecutive_errors = 0;
    }
    io.runtime_state
        .handle(MakerEvent::RecoverySucceeded(spec.recovery_token));
    *io.next_cycle_is_recovery = true;
    // The mutations above must stay synchronous and contiguous: no awaits or
    // output may be inserted between them, so the runtime state, projection,
    // and loop flags advance atomically with respect to the notice below.
    if let Some(note) = &spec.recovered_note {
        emit_reconciliation_state(
            io.output_format,
            io.symbol,
            io.cycle,
            "recovered",
            note.cause,
            note.expected,
            note.observed,
        );
    }
    io.notifier.risk(spec.notice, false).await;
}

/// A buffered or queued order-response signals a genuine correlation failure
/// only when it carried a `request_id` that matched no pending request. A
/// matched accepted placement/cancel or rejected placement remains correlated,
/// even when processed while the runtime is already frozen for an unrelated
/// account event. A matched rejected cancellation is classified separately and
/// fails closed because the maker cannot assume the order is gone.
fn order_response_correlation_failed(matched: bool, request_id: Option<&str>) -> bool {
    request_id.is_some() && !matched
}

#[derive(Debug, PartialEq, Eq)]
struct CancelRejection {
    request_id: String,
    code: i64,
    message: String,
}

fn order_response_failure(
    outcome: &std::result::Result<bool, CancelRejection>,
    request_id: Option<&str>,
    runtime_state: &mut MakerState,
) -> Option<String> {
    match outcome {
        Ok(matched) if order_response_correlation_failed(*matched, request_id) => {
            let request_id = request_id.expect("correlation failure requires request ID");
            runtime_state.handle(MakerEvent::OrderResponseUnmatched {
                request_id: request_id.to_string(),
            });
            Some(format!(
                "order-response correlation failed closed: unexpected request_id={request_id}"
            ))
        }
        Err(rejection) => {
            let detail = maker::order_cancel_rejection_reason(
                &rejection.request_id,
                rejection.code,
                &rejection.message,
            );
            runtime_state.handle(MakerEvent::OrderCancelRejected {
                request_id: rejection.request_id.clone(),
                code: rejection.code,
                message: rejection.message.clone(),
            });
            Some(detail)
        }
        _ => None,
    }
}

fn observe_order_ack(
    tracker: Option<&mut maker::OrderLatencyTracker>,
    started: Option<std::time::Instant>,
    request_id: Option<&str>,
    accepted: bool,
) {
    let (Some(tracker), Some(started), Some(request_id)) = (tracker, started, request_id) else {
        return;
    };
    let at_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if let Err(error) = tracker.mark_ack(request_id, at_ms, accepted) {
        eprintln!("⚠️ order latency ack observation unavailable: {error}");
    }
}

fn invalidate_session_latency(session: &mut LiveSession) {
    let generation = session.projection.generation();
    let at_ms = u64::try_from(session.latency_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if let Err(error) = session
        .order_latency
        .invalidate_generation(generation, at_ms)
    {
        eprintln!("⚠️ order latency generation invalidation unavailable: {error}");
    }
}

#[cfg(test)]
pub(super) fn apply_order_responses(
    receiver: &mut tokio::sync::mpsc::Receiver<OrderResponse>,
    projection: &mut MakerAccountProjection,
    runtime_state: &mut MakerState,
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    price_decimals: u32,
) -> Result<()> {
    apply_order_responses_observed(
        receiver,
        projection,
        runtime_state,
        OrderResponseObservation {
            output_format,
            symbol,
            cycle,
            price_decimals,
            latency: None,
            latency_started: None,
        },
    )
}

struct OrderResponseObservation<'a> {
    output_format: OutputFormat,
    symbol: &'a str,
    cycle: u64,
    price_decimals: u32,
    latency: Option<&'a mut maker::OrderLatencyTracker>,
    latency_started: Option<std::time::Instant>,
}

fn apply_order_responses_observed(
    receiver: &mut tokio::sync::mpsc::Receiver<OrderResponse>,
    projection: &mut MakerAccountProjection,
    runtime_state: &mut MakerState,
    mut observation: OrderResponseObservation<'_>,
) -> Result<()> {
    loop {
        let response = match receiver.try_recv() {
            Ok(response) => response,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(()),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!(
                    "order-response stream disconnected; refusing further live orders"
                ));
            }
        };
        let request_id = response.request_id.clone();
        observe_order_ack(
            observation.latency.as_deref_mut(),
            observation.latency_started,
            request_id.as_deref(),
            response.accepted(),
        );
        let outcome = apply_order_response(
            response,
            projection,
            observation.output_format,
            observation.symbol,
            observation.cycle,
            observation.price_decimals,
        );
        if let Some(error) = order_response_failure(&outcome, request_id.as_deref(), runtime_state)
        {
            return Err(anyhow::anyhow!(error));
        }
    }
}

fn apply_order_response(
    response: OrderResponse,
    projection: &mut MakerAccountProjection,
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    price_decimals: u32,
) -> std::result::Result<bool, CancelRejection> {
    let Some(request_id) = response.request_id.as_deref() else {
        return Ok(false);
    };
    let Some(pending) = projection.pending_request(request_id).cloned() else {
        if let Some(resolution) = projection.completed_request_resolution(request_id) {
            return Ok(resolution.accepts_response(response.accepted()));
        }
        return Ok(false);
    };
    let generation = projection.generation();
    match pending {
        ProjectionPendingRequest::Cancel(_) => {
            if !response.accepted() {
                return Err(CancelRejection {
                    request_id: request_id.to_string(),
                    code: response.code,
                    message: response.message,
                });
            }
            projection.apply(
                generation,
                AccountProjectionEvent::CancelResolved {
                    request_id: request_id.to_string(),
                },
            );
        }
        ProjectionPendingRequest::Place(place) => {
            if response.accepted() {
                projection.apply(
                    generation,
                    AccountProjectionEvent::PlaceAccepted {
                        request_id: request_id.to_string(),
                    },
                );
            } else {
                projection.apply(
                    generation,
                    AccountProjectionEvent::PlaceRejected {
                        request_id: request_id.to_string(),
                    },
                );
                output::log_maker_event(output::MakerLogEvent {
                    output_format,
                    symbol,
                    cycle,
                    action: "place_rejected_async",
                    side: place.side,
                    level: place.level,
                    price: place.price,
                    price_decimals,
                    detail: &response.message,
                });
            }
        }
    }
    Ok(true)
}

struct AccountEventContext<'a> {
    symbol: &'a str,
    run_order_prefix: &'a str,
    mark: f64,
    cycle: u64,
    output_format: OutputFormat,
}

struct AccountEventState<'a> {
    ledger: &'a mut MakerLedger,
    stats: &'a mut MakerStats,
    projection: &'a mut MakerAccountProjection,
}

#[derive(Debug, Default)]
struct AccountEventOutcome {
    fills: u64,
    latest_position: Option<f64>,
    exit_fill_observed: bool,
    balance_changed: bool,
    requires_order_reconciliation: bool,
    effective_request_ids: Vec<String>,
    fill_order_ids: Vec<u64>,
}

impl AccountEventOutcome {
    fn merge(&mut self, other: Self) {
        self.fills += other.fills;
        if other.latest_position.is_some() {
            self.latest_position = other.latest_position;
        }
        self.exit_fill_observed |= other.exit_fill_observed;
        self.balance_changed |= other.balance_changed;
        self.requires_order_reconciliation |= other.requires_order_reconciliation;
        self.effective_request_ids
            .extend(other.effective_request_ids);
        self.fill_order_ids.extend(other.fill_order_ids);
    }
}

fn schedule_account_balance_refresh(
    requested: &mut bool,
    account_alerts_enabled: bool,
    poll: &mut LiveAccountPollState,
    now: std::time::Instant,
) -> bool {
    if !std::mem::take(requested) || !account_alerts_enabled {
        return false;
    }
    poll.request_balance_refresh(now);
    true
}

/// Loop-carried state and context an account outcome feeds back into, borrowed
/// for the duration of one [`absorb_account_outcome`] call.
struct OutcomeSink<'a> {
    total_fills: &'a mut u64,
    balance_refresh_requested: &'a mut bool,
    inventory_exit_pending: &'a mut bool,
    notifier: &'a MakerNotifier,
    position_alert_anchor: &'a mut PositionAlertAnchor,
    expected_position: f64,
    max_position: f64,
    inventory_exit_pct: f64,
    qty_tolerance: f64,
    symbol: &'a str,
    cycle: u64,
    order_latency: Option<&'a mut maker::OrderLatencyTracker>,
    latency_started: Option<std::time::Instant>,
}

/// Fold one account-event outcome into the loop totals: accumulate fills, clear
/// the pending inventory exit once its fill is observed, and emit a
/// position-jump alert. Returns the latest observed position (if any) so the
/// caller can apply its own mismatch bookkeeping — the one part that legitimately
/// differs between the cycle, replan, and reconciliation paths.
async fn absorb_account_outcome(
    outcome: AccountEventOutcome,
    mut sink: OutcomeSink<'_>,
) -> Option<f64> {
    if let (Some(tracker), Some(started)) =
        (sink.order_latency.as_deref_mut(), sink.latency_started)
    {
        let at_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        for order_id in &outcome.fill_order_ids {
            if let Err(error) = tracker.record_fill_after_cancel_order(*order_id, at_ms) {
                eprintln!("⚠️ fill-after-cancel observation unavailable: {error}");
            }
        }
        for request_id in &outcome.effective_request_ids {
            if let Err(error) = tracker.mark_effective(request_id, at_ms) {
                eprintln!("⚠️ order latency effective observation unavailable: {error}");
            }
        }
    }
    *sink.total_fills += outcome.fills;
    *sink.balance_refresh_requested |= outcome.balance_changed;
    if outcome.exit_fill_observed {
        *sink.inventory_exit_pending = false;
    }
    let position = outcome.latest_position;
    if let Some(position) = position {
        sink.notifier
            .position_jump(
                sink.position_alert_anchor,
                PositionChange {
                    observed: position,
                    expected: sink.expected_position,
                    max_position: sink.max_position,
                    inventory_exit_pct: sink.inventory_exit_pct,
                    qty_tolerance: sink.qty_tolerance,
                    symbol: sink.symbol,
                    cycle: sink.cycle,
                },
            )
            .await;
    }
    position
}
fn apply_account_events(
    receiver: &mut tokio::sync::mpsc::Receiver<AccountEvent>,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<AccountEventOutcome> {
    let mut outcome = AccountEventOutcome::default();
    loop {
        let event = match receiver.try_recv() {
            Ok(event) => event,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                return Ok(outcome);
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!("authenticated account stream disconnected"));
            }
        };
        outcome.merge(apply_account_event(event, state, context)?);
    }
}

fn account_event_invalidates_cycle(event: &AccountEvent) -> bool {
    matches!(
        event,
        AccountEvent::Position(_)
            | AccountEvent::Trade(_)
            | AccountEvent::Disconnected { .. }
            | AccountEvent::Error { .. }
    )
}

/// A position/trade event that arrives while a cycle owns in-flight work must
/// still take the reconciliation cleanup path even when its observed position
/// already agrees with the ledger. The reducer has queued `AbortInFlight` and
/// `Cleanup` at that point; skipping straight to the next cycle would leave
/// those effects pending and turn the abort into a spurious fail-safe stop.
fn reconciliation_error_for_cycle(
    expected: f64,
    mismatch: Option<f64>,
    account_position_mismatch: Option<f64>,
    cycle_invalidated_by_account: bool,
) -> Option<PositionReconciliationError> {
    if let Some(observed) = mismatch.or(account_position_mismatch) {
        Some(PositionReconciliationError::position_mismatch(
            expected, observed,
        ))
    } else if cycle_invalidated_by_account {
        Some(PositionReconciliationError::cycle_invalidation(expected))
    } else {
        None
    }
}

fn reconciliation_recovery_admission(
    reconciliation: &PositionReconciliationError,
    breaker: &mut maker::RecoveryCircuitBreaker,
    now_secs: u64,
) -> Option<maker::RecoveryAdmission> {
    reconciliation
        .cause
        .recovery_trigger()
        .meters_circuit()
        .then(|| breaker.admit(now_secs))
}

fn market_update_requires_replan(
    previous_mark: f64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    resting: &[RestingQuote],
    refresh_bps: f64,
    max_divergence_bps: f64,
) -> bool {
    // Crossed book and mark/mid divergence share the exact skip rules the
    // strategy applies in preflight_cycle; route through the same predicate so
    // the replan trigger cannot drift from it.
    maker::bps_diff(mark, previous_mark) > refresh_bps
        || maker::touch_skip(mark, best_bid, best_ask, max_divergence_bps).is_some()
        || maker::resting_quotes_would_cross(resting, best_bid, best_ask)
}

fn apply_account_event(
    event: AccountEvent,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<AccountEventOutcome> {
    output::emit_account_event_lag(context.output_format, &event, context.symbol, context.cycle);
    match event {
        AccountEvent::Connected { epoch } => {
            if epoch != state.projection.generation() {
                return Err(anyhow::anyhow!(
                    "stale account-stream generation {epoch}; current projection generation is {}",
                    state.projection.generation()
                ));
            }
            Ok(AccountEventOutcome::default())
        }
        AccountEvent::Order(update) => {
            let mut fills = Vec::new();
            let observation = model::stream_order_observation(&update)?;
            let exit_fill_observed = ledger::apply_order_update(
                state.ledger,
                &update,
                context.symbol,
                context.run_order_prefix,
                state.stats,
                &mut fills,
            )?;
            let generation = state.projection.generation();
            let projection_outcome = state.projection.apply(
                generation,
                AccountProjectionEvent::OrderObserved(observation),
            );
            let requires_order_reconciliation = projection_outcome.unknown_current_run_order;
            for fill in &fills {
                if let Some(order_id) = fill.order_id {
                    state.projection.apply(
                        generation,
                        AccountProjectionEvent::TradeApplied {
                            order_id,
                            qty: fill.qty,
                        },
                    );
                }
                emit_live_fill(fill, context.symbol, context.cycle, context.output_format);
            }
            Ok(AccountEventOutcome {
                fills: fills.len() as u64,
                latest_position: None,
                exit_fill_observed,
                balance_changed: false,
                requires_order_reconciliation,
                effective_request_ids: projection_outcome
                    .effective_request_id
                    .into_iter()
                    .collect(),
                fill_order_ids: fills.iter().filter_map(|fill| fill.order_id).collect(),
            })
        }
        AccountEvent::Position(update) => {
            if !update.symbol.eq_ignore_ascii_case(context.symbol) {
                return Ok(AccountEventOutcome::default());
            }
            let qty =
                model::signed_position_quantity(&update.qty, update.side).map_err(|error| {
                    anyhow::anyhow!("account position update has invalid qty: {error}")
                })?;
            let generation = state.projection.generation();
            state.projection.apply(
                generation,
                AccountProjectionEvent::PositionObserved { position: qty },
            );
            Ok(AccountEventOutcome {
                fills: 0,
                latest_position: Some(qty),
                exit_fill_observed: false,
                balance_changed: false,
                requires_order_reconciliation: false,
                effective_request_ids: Vec::new(),
                fill_order_ids: Vec::new(),
            })
        }
        AccountEvent::Trade(trade) => {
            let mut fills = Vec::new();
            let exit_fill_observed = ledger::apply_account_trade(
                state.ledger,
                trade,
                context.symbol,
                context.mark,
                state.stats,
                &mut fills,
            )?;
            let generation = state.projection.generation();
            for fill in &fills {
                if let Some(order_id) = fill.order_id {
                    state.projection.apply(
                        generation,
                        AccountProjectionEvent::TradeApplied {
                            order_id,
                            qty: fill.qty,
                        },
                    );
                }
                emit_live_fill(fill, context.symbol, context.cycle, context.output_format);
            }
            Ok(AccountEventOutcome {
                fills: fills.len() as u64,
                latest_position: None,
                exit_fill_observed,
                balance_changed: false,
                requires_order_reconciliation: false,
                effective_request_ids: Vec::new(),
                fill_order_ids: fills.iter().filter_map(|fill| fill.order_id).collect(),
            })
        }
        // A balance update only signals that the REST-backed margin snapshot
        // the alert floors read is now stale; flag a refresh. The raw wallet
        // fields are not projected — nothing reads a projected balance.
        AccountEvent::Balance(_update) => Ok(AccountEventOutcome {
            balance_changed: true,
            ..AccountEventOutcome::default()
        }),
        AccountEvent::Disconnected { reason } | AccountEvent::Error { reason } => Err(
            anyhow::anyhow!("authenticated account stream unhealthy: {reason}"),
        ),
    }
}

fn accounting_position_mismatch(
    expected_position: f64,
    stats_position: f64,
    qty_tolerance: f64,
) -> bool {
    let delta = (stats_position - expected_position).abs();
    // Fail closed: a non-finite delta (NaN from a poisoned position) would make
    // a bare `>` comparison false and silently pass the invariant, so treat any
    // non-finite value as a mismatch.
    !delta.is_finite() || delta > qty_tolerance
}

fn recovery_circuit_detail(admission: maker::RecoveryAdmission) -> String {
    let (incidents, limit, window_secs) = match admission {
        maker::RecoveryAdmission::Admitted {
            incidents,
            limit,
            window_secs,
        }
        | maker::RecoveryAdmission::CircuitOpen {
            incidents,
            limit,
            window_secs,
        } => (incidents, limit, window_secs),
    };
    format!("rolling recovery circuit {incidents}/{limit} incident(s) in {window_secs}s")
}

async fn accounting_invariant_exit(
    notifier: &MakerNotifier,
    symbol: &str,
    cycle: u64,
    expected_position: f64,
    stats_position: f64,
    qty_tolerance: f64,
) -> Option<MakerExit> {
    if !accounting_position_mismatch(expected_position, stats_position, qty_tolerance) {
        return None;
    }
    let detail = format!(
        "stats position {stats_position:+.8} differs from ledger expected {expected_position:+.8} beyond tolerance {qty_tolerance:.8}"
    );
    notifier
        .risk(
            RiskNotice {
                kind: "accounting_invariant",
                severity: "critical",
                event: "mismatch",
                message: &detail,
                symbol,
                cycle,
                position_before: None,
                position_after: Some(expected_position),
                expected: Some(expected_position),
                observed: Some(stats_position),
            },
            true,
        )
        .await;
    Some(MakerExit::AccountingInvariant(detail))
}

struct ShutdownReport<'a> {
    live: bool,
    output_format: OutputFormat,
    symbol: &'a str,
    cfg: &'a MakerConfig,
    client: &'a StandXClient,
    notifier: &'a MakerNotifier,
    ledger: &'a MakerLedger,
    stats: &'a MakerStats,
    breaker: &'a VolBreaker,
    exit: MakerExit,
    cycle: u64,
    total_places: u64,
    total_cancels: u64,
    total_holds: u64,
    total_fills: u64,
    total_halted: u64,
    sim_position: f64,
    last_mark: Option<f64>,
    feed_handle: Option<tokio::task::JoinHandle<()>>,
    account_stream_handle: Option<tokio::task::JoinHandle<()>>,
    order_response_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Abort feed/stream tasks, cancel any residual maker orders, print the human
/// summary, and deliver the stopped-lifecycle notifications. Runs on every
/// exit path; returns the process result (fail-safe error or clean Ok).
async fn shutdown_report(report: ShutdownReport<'_>) -> Result<()> {
    let ShutdownReport {
        live,
        output_format,
        symbol,
        cfg,
        client,
        notifier,
        ledger,
        stats,
        breaker,
        exit,
        cycle,
        total_places,
        total_cancels,
        total_holds,
        total_fills,
        total_halted,
        sim_position,
        last_mark,
        feed_handle,
        account_stream_handle,
        order_response_handle,
    } = report;
    if let Some(handle) = feed_handle {
        handle.abort();
    }
    if let Some(handle) = account_stream_handle {
        handle.abort();
    }
    let final_position = if live {
        ledger.expected_position
    } else {
        stats.position()
    };
    if output_format == OutputFormat::Table {
        println!(
            "\n👋 Stopping maker (ran {} cycles: {} places, {} cancels, {} holds)",
            cycle, total_places, total_cancels, total_holds
        );
        let pnl_note = match last_mark {
            Some(m) => format!(
                " | PnL {:+.2} (mark-to-market)",
                stats.pnl(final_position, m)
            ),
            None => String::new(),
        };
        println!(
            "   {} fills | uptime {:.0}% | max pos {} | avg capture {:.1}bps{}",
            total_fills,
            stats.uptime_pct(),
            maker::format_decimals(stats.max_abs_position, cfg.qty_decimals),
            stats.avg_spread_capture_bps(),
            pnl_note
        );
        if breaker.enabled() {
            println!("   vol breaker: {} cycles halted", total_halted);
        }
        if !live {
            println!(
                "   paper sim: ending position {}",
                maker::format_decimals(sim_position, cfg.qty_decimals)
            );
        }
    }
    if let Some(handle) = order_response_handle {
        handle.abort();
    }
    // Do not return early on cleanup failure: operators need the stopped
    // lifecycle alert most when residual maker orders may still be live.
    let cleanup_error = if live {
        cancel_maker_orders_with_retry(client, symbol, 3, output_format)
            .await
            .err()
    } else {
        None
    };

    // Notify stop on every exit path. Await delivery so the message lands
    // before the process exits.
    let reason = exit.lifecycle_reason();
    let pnl_str = last_mark
        .map(|m| format!("{:+.2}", stats.pnl(final_position, m)))
        .unwrap_or_else(|| "n/a".to_string());
    let cleanup_note = cleanup_error.as_ref().map_or_else(String::new, |error| {
        format!(" | ⚠️ cleanup failed: {error}")
    });
    if let Some(error) = cleanup_error.as_ref() {
        let message = format!("maker cleanup failed or left residual orders: {error}");
        notifier
            .risk(
                RiskNotice {
                    kind: "maker_cleanup",
                    severity: "critical",
                    event: "residual_orders",
                    message: &message,
                    symbol,
                    cycle,
                    position_before: None,
                    position_after: None,
                    expected: Some(ledger.expected_position),
                    observed: None,
                },
                true,
            )
            .await;
    }
    if !matches!(&exit, MakerExit::CtrlC) {
        notifier
            .risk(
                RiskNotice {
                    kind: "fail_safe",
                    severity: "critical",
                    event: "stopped",
                    message: &reason,
                    symbol,
                    cycle,
                    position_before: None,
                    position_after: None,
                    expected: Some(ledger.expected_position),
                    observed: None,
                },
                true,
            )
            .await;
    }
    notifier
        .lifecycle(
            "stopped",
            &format!(
                "🔴 maker stopped ({}) — {} | {} cycles, {} fills, uptime {:.0}%, PnL {}{}",
                reason,
                symbol,
                cycle,
                total_fills,
                stats.uptime_pct(),
                pnl_str,
                cleanup_note,
            ),
            symbol,
            true,
        )
        .await;

    if let Some(error) = cleanup_error {
        // A residual-order cleanup failure is an intentional fail-safe stop
        // that needs a human, not an automatic restart.
        return Err(anyhow::Error::new(FailSafeShutdown {
            message: format!(
                "maker stopped (fail-safe) but maker-owned order cleanup failed: {error}"
            ),
        }));
    }

    match exit.terminal_error() {
        Some(message) => Err(anyhow::Error::new(FailSafeShutdown { message })),
        None => Ok(()),
    }
}

pub(super) async fn run_maker(
    symbol: String,
    args: MakerRunArgs,
    output_format: OutputFormat,
) -> Result<()> {
    // Validate args, resolve config, and (in live mode) run the clean-start
    // handshake into a fully-initialized session before the quoting loop.
    let MakerStartup {
        client,
        cfg,
        symbol,
        notifier,
        qty_tolerance,
        run_order_prefix,
        starting_position,
        baseline_mark,
        session_started_at,
        mut live_session,
    } = run_startup(symbol, &args, output_format).await?;

    let mode = if args.live { "LIVE" } else { "PAPER" };
    if output_format == OutputFormat::Table {
        println!("┌──────────────────────────────────────────────────────────┐");
        println!("│ standx maker — {} mode on {}", mode, symbol);
        println!(
            "│ spread {}bps | band {}bps | refresh {}bps | {} level(s)",
            cfg.spread_bps, cfg.band_bps, cfg.refresh_bps, cfg.levels
        );
        println!(
            "│ size {} | max-position {} | interval {}s",
            cfg.size, cfg.max_position, args.interval
        );
        if cfg.skew_bps > 0.0 {
            println!(
                "│ inventory skew {}bps (live only; paper holds no position)",
                cfg.skew_bps
            );
        }
        if args.inventory_exit_pct > 0.0 {
            println!(
                "│ active exit: {}% of max, reduce-only chunks of {} (live only)",
                args.inventory_exit_pct, args.inventory_exit_qty
            );
        }
        println!(
            "│ ticks: price {}dp, qty {}dp | min qty {}",
            cfg.price_decimals, cfg.qty_decimals, cfg.min_order_qty
        );
        if !args.live {
            println!("│ paper mode: no real orders; fills are simulated when the");
            println!("│ touch crosses a quote, so position & skew move. --live for real.");
        } else {
            println!(
                "│ ⚠️  LIVE: the bot manages only {} orders on {}",
                MAKER_CL_ORD_ID_PREFIX, symbol
            );
            println!("│ manual/API orders are preserved and ignored.");
            println!(
                "│ order-response recovery: {} attempt(s), {}s base backoff",
                args.order_response_reconnect_attempts, args.order_response_reconnect_backoff
            );
            println!(
                "│ order request deadline: {}s to correlated ACK/account visibility",
                ORDER_REQUEST_TIMEOUT.as_secs()
            );
            println!(
                "│ account-stream recovery: {} attempt(s), {}s base backoff",
                args.account_stream_reconnect_attempts, args.account_stream_reconnect_backoff
            );
            println!(
                "│ transport recovery circuit: {} incident(s) / {}s rolling window",
                args.recovery_incidents_per_window, args.recovery_window_secs
            );
        }
        if args.no_ws {
            println!("│ feed: REST polling (--no-ws)");
        } else {
            println!(
                "│ feed: websocket (REST fallback) | divergence guard {}bps",
                args.max_divergence_bps
            );
        }
        if args.vol_pause_bps > 0.0 {
            println!(
                "│ vol breaker: halt at {}bps range / {} cycles (resume < {}bps)",
                args.vol_pause_bps,
                args.vol_window.max(1),
                args.vol_pause_bps / 2.0
            );
        }
        if args.stop_loss > 0.0 {
            println!(
                "│ stop-loss: session PnL -{} → fail-safe shutdown",
                args.stop_loss
            );
        }
        if args.alert_loss > 0.0
            || args.alert_inventory_pct > 0.0
            || args.alert_position_change_pct > 0.0
            || args.alert_uptime > 0.0
            || args.alert_equity_below > 0.0
            || args.alert_margin_below > 0.0
        {
            let mut parts = Vec::new();
            if args.alert_loss > 0.0 {
                parts.push(format!("loss -{}", args.alert_loss));
            }
            if args.alert_inventory_pct > 0.0 {
                parts.push(format!("inv {}%", args.alert_inventory_pct));
            }
            if args.alert_position_change_pct > 0.0 {
                parts.push(format!("position Δ {}%", args.alert_position_change_pct));
            }
            if args.alert_uptime > 0.0 {
                parts.push(format!("uptime {}%", args.alert_uptime));
            }
            if args.alert_equity_below > 0.0 {
                parts.push(format!("equity <{}", args.alert_equity_below));
            }
            if args.alert_margin_below > 0.0 {
                parts.push(format!("margin <{}", args.alert_margin_below));
            }
            let sink = if args.alert_webhook.is_some() {
                format!("stderr + webhook ({:?})", args.alert_webhook_format).to_lowercase()
            } else {
                "stderr".to_string()
            };
            println!("│ risk alerts: {} → {}", parts.join(", "), sink);
        }
        println!("│ Ctrl+C to stop (cancels maker-owned resting orders on exit)");
        println!("└──────────────────────────────────────────────────────────┘");
    }

    // Notify start (fire-and-forget; the process keeps running).
    notifier
        .lifecycle(
            "started",
            &format!(
            "🟢 maker started — {} {} | spread {}bps band {}bps size {} | {} | order-response reconnects {}",
            mode,
            symbol,
            cfg.spread_bps,
            cfg.band_bps,
            cfg.size,
            if args.no_ws { "REST" } else { "WS" },
            if args.live {
                args.order_response_reconnect_attempts
            } else {
                0
            }
            ),
            &symbol,
            false,
        )
    .await;

    // ---- Market feed (WS primary, REST fallback) ----
    let (feed, mut updates, feed_handle) = if args.no_ws {
        (None, None, None)
    } else {
        let (state, rx, handle) = spawn_market_feed(symbol.clone(), args.verbose);
        (Some(state), Some(rx), Some(handle))
    };

    // ---- Loop state ----
    let mut cycle: u64 = 0;
    let mut resting: Vec<RestingQuote> = Vec::new(); // paper-mode book
    let mut inventory_exit_pending = false;
    let mut ledger = MakerLedger::new(starting_position);
    ledger.enable_performance(baseline_mark)?;
    let performance_started = std::time::Instant::now();
    let performance_epoch_ms = chrono::Utc::now().timestamp_millis();
    let mut position_alert_anchor =
        PositionAlertAnchor::new(starting_position, args.alert_position_change_pct);
    let mut consecutive_errors: u32 = 0;
    let mut total_places: u64 = 0;
    let mut total_cancels: u64 = 0;
    let mut total_holds: u64 = 0;
    let mut total_fills: u64 = 0;
    let mut total_halted: u64 = 0;
    // Observation only: classify the first successfully committed cycle after
    // bounded recovery without feeding the flag into strategy or safety.
    let mut next_cycle_is_recovery = false;
    let mut sim_position: f64 = 0.0; // paper-mode simulated inventory
    let mut stats = if args.live {
        MakerStats::with_inventory_baseline(starting_position, baseline_mark)
    } else {
        MakerStats::default()
    };
    let mut breaker = VolBreaker::new(args.vol_window.max(1) as usize, args.vol_pause_bps);
    let mut alerts =
        AlertMonitor::new(args.alert_loss, args.alert_inventory_pct, args.alert_uptime)
            .with_account_floors(args.alert_equity_below, args.alert_margin_below);
    let mut last_mark: Option<f64> = None;
    let mut last_src: Option<&'static str> = None;
    let recovery_clock_started = std::time::Instant::now();
    // One rolling budget shared across abnormal automatic recoveries —
    // account-stream reconnect, order-response reconnect, and genuine
    // position/projection mismatch. A normal account event that invalidates an
    // in-flight cycle still performs cleanup and reconciliation, but is not an
    // incident: otherwise ordinary fills eventually exhaust the live budget.
    let mut recovery_breaker = maker::RecoveryCircuitBreaker::new(
        args.recovery_incidents_per_window,
        args.recovery_window_secs,
    );
    let mut account_position_mismatch: Option<f64> = None;
    let mut account_order_reconciliation_required = false;
    // A wallet-level WS balance update cannot be substituted for the unified
    // REST equity/margin model. Coalesce updates into one immediate
    // authoritative refresh on the next cycle when account floors are active.
    let mut account_balance_refresh_requested = false;
    // JWT expiry monitor: highest severity already alerted, plus a throttle so
    // credentials are only reloaded from disk/env periodically.
    let mut token_expiry_alerted = TokenExpiryLevel::Ok;
    let mut last_token_expiry_check: Option<std::time::Instant> = None;
    let mut balance_floor_parse_warned = false;
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);

    // Long-lived Ctrl+C listener. Tokio's first ctrl_c() call permanently
    // replaces the process SIGINT handler, so a per-select future silently
    // drops presses that arrive between selects (and the process no longer
    // dies on SIGINT either). Latch presses into a watch channel every wait
    // point can observe.
    let (ctrl_c_tx, mut ctrl_c_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        while signal::ctrl_c().await.is_ok() {
            let _ = ctrl_c_tx.send(true);
        }
    });

    let exit = 'main: loop {
        if args.live {
            // JWT expiry monitor. There is no renewal endpoint, so we can only
            // warn: escalate through Warning → Critical and alert once per band.
            let due = last_token_expiry_check
                .map(|last| last.elapsed() >= TOKEN_EXPIRY_CHECK_INTERVAL)
                .unwrap_or(true);
            if due {
                last_token_expiry_check = Some(std::time::Instant::now());
                if let Ok(creds) = Credentials::load() {
                    let remaining = creds.remaining_seconds();
                    let level = token_expiry_level(
                        remaining,
                        TOKEN_EXPIRY_WARN_SECS,
                        TOKEN_EXPIRY_CRITICAL_SECS,
                    );
                    if level > token_expiry_alerted {
                        token_expiry_alerted = level;
                        let (severity, event) = match level {
                            TokenExpiryLevel::Critical => ("critical", "token_expiry_critical"),
                            _ => ("warning", "token_expiry_warning"),
                        };
                        let minutes = remaining / 60;
                        let message = format!(
                            "auth token expires in ~{minutes}m ({}); no renewal endpoint — run 'standx auth login' before it lapses or the bot will halt",
                            creds.expires_at_string()
                        );
                        notifier
                            .risk(
                                RiskNotice {
                                    kind: "token_expiry",
                                    severity,
                                    event,
                                    message: &message,
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: None,
                                    expected: None,
                                    observed: None,
                                },
                                false,
                            )
                            .await;
                    }
                }
            }
        }
        if let Some(session) = live_session.as_mut() {
            if !session.account_stream_health.is_healthy() {
                let detail = session
                    .account_stream_health
                    .failure_reason()
                    .unwrap_or_else(|| {
                        "account stream became unhealthy without a recorded reason".to_string()
                    });
                let message = format!(
                    "account stream unavailable; placements frozen and cleanup starting: {detail}"
                );
                // Freeze immediately: no further cycle can place while the
                // authoritative account stream is unavailable. The stale
                // receiver/health stay in place until the reconnect replaces
                // them; every failure path below exits the loop.
                let recovery_token = match freeze_and_cleanup_for_recovery(
                    &mut RecoveryIo {
                        runtime_state: &mut runtime_state,
                        notifier: &notifier,
                        client: &client,
                        session: Some(&mut *session),
                        resting: &mut resting,
                        inventory_exit_pending: &mut inventory_exit_pending,
                        consecutive_errors: &mut consecutive_errors,
                        next_cycle_is_recovery: &mut next_cycle_is_recovery,
                        symbol: &symbol,
                        cycle,
                        output_format,
                    },
                    FreezeSpec {
                        target: RecoveryTarget::AccountStream,
                        trigger: MakerEvent::AccountStreamDisconnected(detail.clone()),
                        cleanup_effect_stop: EffectFailureStop::CleanupFailure,
                        recovery_effect_stop: EffectFailureStop::PositionReconciliation,
                        cleanup_failure_prefix: format!("account stream disconnected ({detail}); "),
                        cleanup_failed_exit: MakerExit::PositionReconciliation,
                        notice: RiskNotice {
                            kind: "account_stream",
                            severity: "warning",
                            event: "disconnected_frozen",
                            message: &message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        frozen_note: None,
                        abort_account_stream_handle: true,
                        projection_reset: ProjectionReset::PreservePendingAcks,
                    },
                )
                .await
                {
                    Ok(token) => token,
                    Err(exit) => break exit,
                };

                if args.account_stream_reconnect_attempts == 0 {
                    break recovery_failed_exit(
                        &mut runtime_state,
                        recovery_token,
                        format!("account stream disconnected ({detail}); reconnect disabled"),
                    );
                }
                let admission = recovery_breaker.admit(recovery_clock_started.elapsed().as_secs());
                if !admission.is_admitted() {
                    break recovery_failed_exit(
                        &mut runtime_state,
                        recovery_token,
                        format!(
                            "account stream disconnected ({detail}); {}; refusing further live orders",
                            recovery_circuit_detail(admission)
                        ),
                    );
                }

                let (mut events, health, handle) = match reconnect_account_stream(
                    &mut session.account_stream_epoch,
                    args.account_stream_reconnect_attempts,
                    args.account_stream_reconnect_backoff,
                    &mut ctrl_c_rx,
                )
                .await
                {
                    AccountStreamReconnect::Connected(triple) => triple,
                    AccountStreamReconnect::Interrupted => {
                        runtime_state.handle(MakerEvent::StopRequested(RuntimeStopReason::CtrlC));
                        break 'main take_stop_effect(
                            &mut runtime_state,
                            MakerExit::PositionReconciliation,
                        );
                    }
                    AccountStreamReconnect::Exhausted(reason) => {
                        runtime_state.handle(MakerEvent::RecoveryFailed {
                            token: recovery_token,
                            reason: format!(
                                "account stream disconnected ({detail}); reconnect exhausted: {reason}"
                            ),
                        });
                        break take_stop_effect(
                            &mut runtime_state,
                            MakerExit::PositionReconciliation,
                        );
                    }
                };

                let projection = &mut session.projection;
                projection.reset_after_cleanup_preserving_pending_acks(
                    session.account_stream_epoch,
                    ledger.expected_position,
                );

                let reconnect_outcome = match apply_account_events(
                    &mut events,
                    &mut AccountEventState {
                        ledger: &mut ledger,
                        stats: &mut stats,
                        projection,
                    },
                    &AccountEventContext {
                        symbol: &symbol,
                        run_order_prefix: &run_order_prefix,
                        mark: last_mark.unwrap_or(baseline_mark),
                        cycle,
                        output_format,
                    },
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!("account stream reconnect event validation failed: {error}"),
                        );
                    }
                };
                account_balance_refresh_requested |= reconnect_outcome.balance_changed;
                let mut reconnect_fills = reconnect_outcome.fills;
                let positions = match client.get_positions(Some(&symbol)).await {
                    Ok(positions) => positions,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!("account stream reconnect snapshot failed: {error}"),
                        );
                    }
                };
                let mut observed = match position_for_symbol(&positions, &symbol) {
                    Ok(position) => position,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            error.to_string(),
                        );
                    }
                };

                if (observed - ledger.expected_position).abs() > qty_tolerance {
                    // WS events can lag REST settlement across a reconnect: give
                    // a bounded window to explain the gap with REST trades
                    // (mirrors the in-cycle freeze-path reconciliation) before
                    // failing closed.
                    let mut gap_closed = false;
                    for delay in [500_u64, 1_000, 1_500] {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        match apply_account_events(
                            &mut events,
                            &mut AccountEventState {
                                ledger: &mut ledger,
                                stats: &mut stats,
                                projection,
                            },
                            &AccountEventContext {
                                symbol: &symbol,
                                run_order_prefix: &run_order_prefix,
                                mark: last_mark.unwrap_or(baseline_mark),
                                cycle,
                                output_format,
                            },
                        ) {
                            Ok(outcome) => {
                                reconnect_fills += outcome.fills;
                                account_balance_refresh_requested |= outcome.balance_changed;
                            }
                            Err(error) => {
                                handle.abort();
                                break 'main recovery_failed_exit(
                                    &mut runtime_state,
                                    recovery_token,
                                    format!(
                                        "account stream reconnect event validation failed during REST backfill: {error}"
                                    ),
                                );
                            }
                        }
                        match probe_position_convergence(
                            &client,
                            ReconcileRequest {
                                symbol: &symbol,
                                session_started_at,
                                run_order_prefix: &run_order_prefix,
                                qty_tolerance,
                                mark: last_mark.unwrap_or(baseline_mark),
                            },
                            &mut ledger,
                            &mut stats,
                            &mut reconnect_fills,
                            cycle,
                            output_format,
                        )
                        .await
                        {
                            ConvergenceProbe::Converged { observed: obs } => {
                                observed = obs;
                                gap_closed = true;
                                break;
                            }
                            ConvergenceProbe::Pending { observed: obs } => observed = obs,
                            ConvergenceProbe::SnapshotFailed(error) => eprintln!(
                                "⚠️  account stream reconnect REST trade backfill failed: {error}"
                            ),
                        }
                    }
                    if !gap_closed {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!(
                                "account stream reconnect snapshot expected {:+.8}, observed {:+.8} (REST trade backfill did not close the gap)",
                                ledger.expected_position, observed
                            ),
                        );
                    }
                }

                // A current-run order may surface after the first freeze
                // cleanup while the account stream is authenticating. Require
                // one final authoritative empty-book verification before the
                // recovered stream can resume quoting.
                if let Err(error) =
                    cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await
                {
                    handle.abort();
                    break recovery_failed_exit(
                        &mut runtime_state,
                        recovery_token,
                        format!(
                            "account stream reconnect final maker-book verification failed: {error}"
                        ),
                    );
                }
                session.account_events = events;
                session.account_stream_health = health;
                session.account_stream_handle = handle;
                total_fills += reconnect_fills;
                resume_quoting_after_recovery(
                    &mut RecoveryIo {
                        runtime_state: &mut runtime_state,
                        notifier: &notifier,
                        client: &client,
                        session: Some(&mut *session),
                        resting: &mut resting,
                        inventory_exit_pending: &mut inventory_exit_pending,
                        consecutive_errors: &mut consecutive_errors,
                        next_cycle_is_recovery: &mut next_cycle_is_recovery,
                        symbol: &symbol,
                        cycle,
                        output_format,
                    },
                    ResumeSpec {
                        recovery_token,
                        observed,
                        projection_reset: ProjectionReset::PreservePendingAcks,
                        clear_resting: false,
                        reset_consecutive_errors: false,
                        recovered_note: None,
                        notice: RiskNotice {
                            kind: "account_stream",
                            severity: "resolved",
                            event: "reconnected",
                            message: "account stream reauthenticated; buffered events and REST trades reconciled against the venue position",
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: Some(observed),
                        },
                    },
                )
                .await;
                continue;
            }
            if !session.order_response_health.is_healthy() {
                let detail = session
                    .order_response_health
                    .failure_reason()
                    .unwrap_or_else(|| {
                        "order-response stream became unhealthy without a recorded reason"
                            .to_string()
                    });
                let controlled_fault = detail.starts_with("controlled fault injection");
                // Mirror the account-stream path: the order-response stream was
                // previously silent on the webhook across disconnect/reconnect.
                let disconnect_message =
                    format!("order-response stream unavailable; placements frozen: {detail}");
                let recovery_token = match freeze_and_cleanup_for_recovery(
                    &mut RecoveryIo {
                        runtime_state: &mut runtime_state,
                        notifier: &notifier,
                        client: &client,
                        session: Some(&mut *session),
                        resting: &mut resting,
                        inventory_exit_pending: &mut inventory_exit_pending,
                        consecutive_errors: &mut consecutive_errors,
                        next_cycle_is_recovery: &mut next_cycle_is_recovery,
                        symbol: &symbol,
                        cycle,
                        output_format,
                    },
                    FreezeSpec {
                        target: RecoveryTarget::OrderResponse,
                        trigger: MakerEvent::OrderResponseDisconnected(detail.clone()),
                        cleanup_effect_stop: EffectFailureStop::OrderResponse,
                        recovery_effect_stop: EffectFailureStop::OrderResponse,
                        cleanup_failure_prefix: "order-response ".to_string(),
                        cleanup_failed_exit: MakerExit::OrderResponse,
                        notice: RiskNotice {
                            kind: "order_response",
                            severity: "warning",
                            event: "disconnected_frozen",
                            message: &disconnect_message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        frozen_note: None,
                        abort_account_stream_handle: false,
                        projection_reset: ProjectionReset::DropPendingRequests,
                    },
                )
                .await
                {
                    Ok(token) => token,
                    Err(exit) => break exit,
                };
                let reconnect_unavailable = if controlled_fault {
                    Some("controlled fault injection requires fail-safe shutdown".to_string())
                } else if args.order_response_reconnect_attempts == 0 {
                    Some("safe reconnect is disabled".to_string())
                } else {
                    let admission =
                        recovery_breaker.admit(recovery_clock_started.elapsed().as_secs());
                    (!admission.is_admitted())
                        .then(|| format!("{}; circuit is open", recovery_circuit_detail(admission)))
                };
                if reconnect_unavailable.is_none() {
                    // Abort the stream task in place; the stale halves are
                    // replaced together on success, and every failure path
                    // below exits the loop.
                    session.order_response_handle.abort();
                    match reconnect_order_response(
                        ReconnectRequest {
                            cleanup_client: client.clone(),
                            symbol: &symbol,
                            session_started_at,
                            run_order_prefix: &run_order_prefix,
                            qty_tolerance,
                            mark: last_mark.unwrap_or(baseline_mark),
                            output_format,
                            max_attempts: args.order_response_reconnect_attempts,
                            base_backoff: Duration::from_secs(
                                args.order_response_reconnect_backoff,
                            ),
                            original_failure: &detail,
                            ctrl_c: ctrl_c_rx.clone(),
                        },
                        &mut ledger,
                        &mut stats,
                    )
                    .await
                    {
                        Ok(reconnected) => {
                            total_fills += reconnected.fills.len() as u64;
                            for fill in &reconnected.fills {
                                emit_live_fill(fill, &symbol, cycle, output_format);
                                if let Some(order_id) = fill.order_id {
                                    let at_ms = u64::try_from(
                                        session.latency_started.elapsed().as_millis(),
                                    )
                                    .unwrap_or(u64::MAX);
                                    if let Err(error) = session
                                        .order_latency
                                        .record_fill_after_cancel_order(order_id, at_ms)
                                    {
                                        eprintln!(
                                            "⚠️ fill-after-cancel observation unavailable: {error}"
                                        );
                                    }
                                }
                            }
                            account_balance_refresh_requested |= !reconnected.fills.is_empty();
                            let reconciled_position = reconnected.position;
                            session.order_commands = reconnected.commands;
                            session.order_responses = reconnected.responses;
                            session.order_response_health = reconnected.health;
                            session.order_response_handle = reconnected.handle;
                            // Cleanup verified an empty maker book. The next
                            // cycle rebuilds exchange state before it may place.
                            resume_quoting_after_recovery(
                                &mut RecoveryIo {
                                    runtime_state: &mut runtime_state,
                                    notifier: &notifier,
                                    client: &client,
                                    session: Some(&mut *session),
                                    resting: &mut resting,
                                    inventory_exit_pending: &mut inventory_exit_pending,
                                    consecutive_errors: &mut consecutive_errors,
                                    next_cycle_is_recovery: &mut next_cycle_is_recovery,
                                    symbol: &symbol,
                                    cycle,
                                    output_format,
                                },
                                ResumeSpec {
                                    recovery_token,
                                    observed: reconciled_position,
                                    projection_reset: ProjectionReset::DropPendingRequests,
                                    clear_resting: true,
                                    reset_consecutive_errors: true,
                                    recovered_note: None,
                                    notice: RiskNotice {
                                        kind: "order_response",
                                        severity: "resolved",
                                        event: "reconnected",
                                        message: "order-response stream reconnected; maker book verified empty before quoting resumes",
                                        symbol: &symbol,
                                        cycle,
                                        position_before: None,
                                        position_after: None,
                                        expected: Some(ledger.expected_position),
                                        observed: Some(reconciled_position),
                                    },
                                },
                            )
                            .await;
                            continue;
                        }
                        Err(error) => {
                            if error.downcast_ref::<ReconnectInterrupted>().is_some() {
                                runtime_state
                                    .handle(MakerEvent::StopRequested(RuntimeStopReason::CtrlC));
                                break take_stop_effect(
                                    &mut runtime_state,
                                    MakerExit::OrderResponse,
                                );
                            }
                            if let Some(reconciliation) =
                                error.downcast_ref::<PositionReconciliationError>()
                            {
                                if output_format == OutputFormat::Json {
                                    println!(
                                        "{}",
                                        serde_json::json!({
                                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                                            "symbol": symbol,
                                            "action": "position_reconciliation",
                                            "event": "failed_during_reconnect",
                                            "expected_position": reconciliation.expected,
                                            "observed_position": reconciliation.observed,
                                            "message": "post-reconnect venue position cannot be explained by current-run maker fills",
                                        })
                                    );
                                }
                                let reconciliation_message = format!(
                                    "order-response reconnect failed reconciliation: {error}"
                                );
                                notifier
                                    .risk(
                                        RiskNotice {
                                            kind: "order_response",
                                            severity: "critical",
                                            event: "reconnect_failed",
                                            message: &reconciliation_message,
                                            symbol: &symbol,
                                            cycle,
                                            position_before: None,
                                            position_after: None,
                                            expected: Some(ledger.expected_position),
                                            observed: None,
                                        },
                                        true,
                                    )
                                    .await;
                                runtime_state.handle(MakerEvent::RecoveryFailed {
                                    token: recovery_token,
                                    reason: error.to_string(),
                                });
                                break take_stop_effect(
                                    &mut runtime_state,
                                    MakerExit::PositionReconciliation,
                                );
                            }
                            let reconnect_failed_message = format!(
                                "{detail}; safe reconnect failed: {error}; refusing further live orders"
                            );
                            notifier
                                .risk(
                                    RiskNotice {
                                        kind: "order_response",
                                        severity: "critical",
                                        event: "reconnect_failed",
                                        message: &reconnect_failed_message,
                                        symbol: &symbol,
                                        cycle,
                                        position_before: None,
                                        position_after: None,
                                        expected: Some(ledger.expected_position),
                                        observed: None,
                                    },
                                    true,
                                )
                                .await;
                            runtime_state.handle(MakerEvent::RecoveryFailed {
                                token: recovery_token,
                                reason: reconnect_failed_message,
                            });
                            break take_stop_effect(&mut runtime_state, MakerExit::OrderResponse);
                        }
                    }
                }
                let reconnect_note = reconnect_unavailable
                    .expect("unavailable reconnect must carry a fail-safe reason");
                let refuse_message =
                    format!("{detail}; {reconnect_note}; refusing further live orders");
                notifier
                    .risk(
                        RiskNotice {
                            kind: "order_response",
                            severity: "critical",
                            event: "reconnect_unavailable",
                            message: &refuse_message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        true,
                    )
                    .await;
                runtime_state.handle(MakerEvent::RecoveryFailed {
                    token: recovery_token,
                    reason: refuse_message,
                });
                break take_stop_effect(&mut runtime_state, MakerExit::OrderResponse);
            }
        }
        if let Some(session) = live_session.as_mut() {
            if let Err(error) = apply_order_responses_observed(
                &mut session.order_responses,
                &mut session.projection,
                &mut runtime_state,
                OrderResponseObservation {
                    output_format,
                    symbol: &symbol,
                    cycle,
                    price_decimals: cfg.price_decimals,
                    latency: Some(&mut session.order_latency),
                    latency_started: Some(session.latency_started),
                },
            ) {
                session
                    .order_response_health
                    .mark_unhealthy(error.to_string());
                continue;
            }
            match apply_account_events(
                &mut session.account_events,
                &mut AccountEventState {
                    ledger: &mut ledger,
                    stats: &mut stats,
                    projection: &mut session.projection,
                },
                &AccountEventContext {
                    symbol: &symbol,
                    run_order_prefix: &run_order_prefix,
                    mark: last_mark.unwrap_or(baseline_mark),
                    cycle,
                    output_format,
                },
            ) {
                Ok(outcome) => {
                    account_order_reconciliation_required |= outcome.requires_order_reconciliation;
                    let position = absorb_account_outcome(
                        outcome,
                        OutcomeSink {
                            total_fills: &mut total_fills,
                            balance_refresh_requested: &mut account_balance_refresh_requested,
                            inventory_exit_pending: &mut inventory_exit_pending,
                            notifier: &notifier,
                            position_alert_anchor: &mut position_alert_anchor,
                            expected_position: ledger.expected_position,
                            max_position: cfg.max_position,
                            inventory_exit_pct: args.inventory_exit_pct,
                            qty_tolerance,
                            symbol: &symbol,
                            cycle,
                            order_latency: Some(&mut session.order_latency),
                            latency_started: Some(session.latency_started),
                        },
                    )
                    .await;
                    if let Some(position) = position.filter(|position| {
                        (*position - ledger.expected_position).abs() > qty_tolerance
                    }) {
                        account_position_mismatch = Some(position);
                    }
                }
                Err(error) => {
                    session
                        .account_stream_health
                        .mark_unhealthy(error.to_string());
                    continue;
                }
            }
            schedule_account_balance_refresh(
                &mut account_balance_refresh_requested,
                alerts.account_enabled(),
                &mut session.account_poll,
                std::time::Instant::now(),
            );
            session
                .order_request_deadlines
                .retain_pending(&session.projection);
            if session.order_response_health.is_healthy() {
                if let Some(timeout) = session.order_request_deadlines.timed_out(
                    &session.projection,
                    std::time::Instant::now(),
                    ORDER_REQUEST_TIMEOUT,
                ) {
                    session
                        .order_response_health
                        .mark_unhealthy(order_request_timeout_detail(&timeout));
                    continue;
                }
            }
        }
        if account_position_mismatch
            .is_some_and(|position| (position - ledger.expected_position).abs() <= qty_tolerance)
        {
            account_position_mismatch = None;
        }
        if args.live {
            if let Some(exit) = accounting_invariant_exit(
                &notifier,
                &symbol,
                cycle,
                ledger.expected_position,
                stats.position(),
                qty_tolerance,
            )
            .await
            {
                break 'main exit;
            }
        }

        // Work phase raced against Ctrl+C so a slow API call can be
        // interrupted (mirrors run_watch_loop).
        let mismatch = account_position_mismatch.take();
        let order_reconciliation_required =
            std::mem::take(&mut account_order_reconciliation_required);
        let exit_pending_before = inventory_exit_pending;
        let breaker_halted_before = breaker.halted();
        let recovery_cycle = next_cycle_is_recovery;
        if runtime_state.pending_effect().is_none() {
            runtime_state.handle(MakerEvent::Timer);
        }
        let cycle_work_token = match runtime_state.next_effect() {
            Some(MakerEffect::RunCycle(token)) => token,
            Some(MakerEffect::Stop(reason)) => break reason.into(),
            Some(effect) => {
                break stop_requested_exit(
                    &mut runtime_state,
                    RuntimeStopReason::PositionReconciliation(format!(
                        "runtime emitted unexpected effect before cycle: {effect:?}"
                    )),
                );
            }
            None => continue,
        };
        // Split the live session into disjoint field borrows once per cycle:
        // the pinned `work` future holds the command/health/projection/poll
        // halves for its whole lifetime while the select loop below drains
        // both receivers concurrently, so a plain `live_session.as_mut()` in
        // each place would alias.
        let (
            mut cycle_order_responses,
            mut cycle_account_events,
            cycle_order_commands,
            cycle_order_response_health,
            cycle_account_stream_health,
            mut cycle_projection,
            mut cycle_order_request_deadlines,
            mut cycle_account_poll,
            mut cycle_order_latency,
            cycle_latency_started,
        ) = match live_session.as_mut() {
            Some(LiveSession {
                order_responses,
                order_commands,
                order_response_health,
                account_events,
                account_stream_health,
                projection,
                order_request_deadlines,
                account_poll,
                order_latency,
                latency_started,
                ..
            }) => (
                Some(order_responses),
                Some(account_events),
                Some(&*order_commands),
                Some(&*order_response_health),
                Some(&*account_stream_health),
                Some(projection),
                Some(order_request_deadlines),
                Some(account_poll),
                Some(order_latency),
                Some(*latency_started),
            ),
            None => (None, None, None, None, None, None, None, None, None, None),
        };
        let work = async {
            if let Some(observed) = mismatch {
                return Err(anyhow::Error::new(
                    PositionReconciliationError::position_mismatch(
                        ledger.expected_position,
                        observed,
                    ),
                ));
            }
            if order_reconciliation_required {
                return Err(anyhow::Error::new(
                    PositionReconciliationError::unknown_current_run_order(
                        ledger.expected_position,
                    ),
                ));
            }
            let (mark, best_bid, best_ask, src, market_fallback_reason) =
                market_snapshot(&client, &symbol, feed.as_ref()).await?;
            let result = maker_cycle(
                CycleRequest {
                    client: &client,
                    symbol: &symbol,
                    cfg: &cfg,
                    live: args.live,
                    cycle,
                    mark,
                    best_bid,
                    best_ask,
                    market_source: src,
                    recovery: recovery_cycle,
                    market_fallback_reason,
                    max_divergence_bps: args.max_divergence_bps,
                    inventory_exit_pct: args.inventory_exit_pct,
                    inventory_exit_qty: args.inventory_exit_qty,
                    session_started_at,
                    run_order_prefix: &run_order_prefix,
                    starting_position,
                    output_format,
                    order_commands: cycle_order_commands,
                    order_response_health: cycle_order_response_health,
                    account_stream_health: cycle_account_stream_health,
                    performance_time_ms: performance_epoch_ms.saturating_add(
                        i64::try_from(performance_started.elapsed().as_millis())
                            .unwrap_or(i64::MAX),
                    ),
                },
                CycleState {
                    resting: &mut resting,
                    account_projection: cycle_projection.as_deref_mut(),
                    inventory_exit_pending: &mut inventory_exit_pending,
                    ledger: &mut ledger,
                    sim_position: &mut sim_position,
                    stats: &mut stats,
                    breaker: &mut breaker,
                    order_request_deadlines: cycle_order_request_deadlines.as_deref_mut(),
                    live_account_poll: cycle_account_poll.as_deref_mut(),
                    order_latency: cycle_order_latency.as_deref_mut(),
                    latency_started: cycle_latency_started,
                },
            )
            .await?;
            Ok::<_, anyhow::Error>((
                result.places,
                result.cancels,
                result.holds,
                result.fills,
                mark,
                src,
                market_fallback_reason,
                breaker.halted(),
                inventory_exit_pending,
                result.balance,
            ))
        };
        // Order lifecycle and balance events are buffered so a cycle's own
        // acknowledgement cannot tear apart a multi-order plan. Position,
        // trade, and stream-failure events can change risk or invalidate the
        // plan. They freeze the reducer before this future is dropped so the
        // queued Cleanup effect compensates for any request that may already
        // have reached the venue.
        let mut buffered_account: Vec<AccountEvent> = Vec::new();
        let mut buffered_orders: Vec<OrderResponse> = Vec::new();
        let mut cycle_invalidated_by_account = false;
        // Scope the pinned work future so it (and its ledger/pending borrows)
        // is dropped once it resolves, before the buffered events are applied.
        let cycle_result = {
            tokio::pin!(work);
            loop {
                let account_during_work = async {
                    match cycle_account_events.as_deref_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                };
                let order_during_work = async {
                    match cycle_order_responses.as_deref_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    biased;
                    _ = ctrl_c_latched(&mut ctrl_c_rx) => {
                        runtime_state.handle(MakerEvent::StopRequested(RuntimeStopReason::CtrlC));
                        break 'main take_stop_effect(&mut runtime_state, MakerExit::PositionReconciliation);
                    },
                    event = account_during_work => {
                        let Some(event) = event else {
                            let reason = "authenticated account stream disconnected during cycle".to_string();
                            runtime_state.handle(MakerEvent::AccountStreamDisconnected(reason.clone()));
                            if let Some(health) = cycle_account_stream_health {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        let invalidates = account_event_invalidates_cycle(&event);
                        buffered_account.push(event);
                        if invalidates {
                            runtime_state.handle(MakerEvent::CycleInvalidated {
                                reason: "account state changed during maker cycle".to_string(),
                            });
                            cycle_invalidated_by_account = true;
                            break None;
                        }
                    },
                    response = order_during_work => {
                        let Some(response) = response else {
                            let reason = "order-response stream disconnected during cycle".to_string();
                            runtime_state.handle(MakerEvent::OrderResponseDisconnected(reason.clone()));
                            if let Some(health) = cycle_order_response_health {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        buffered_orders.push(response);
                    },
                    result = &mut work => break Some(result),
                }
            }
        };
        // The buffers are only fed from live-session receivers, so both are
        // empty in paper mode.
        if let Some(session) = live_session.as_mut() {
            if cycle_invalidated_by_account {
                while let Ok(event) = session.account_events.try_recv() {
                    buffered_account.push(event);
                }
            }
            // Apply the events buffered during work, ordering order-responses
            // before account events to mirror the top-of-loop drain.
            for response in buffered_orders {
                let request_id = response.request_id.clone();
                observe_order_ack(
                    Some(&mut session.order_latency),
                    Some(session.latency_started),
                    request_id.as_deref(),
                    response.accepted(),
                );
                let outcome = apply_order_response(
                    response,
                    &mut session.projection,
                    output_format,
                    &symbol,
                    cycle,
                    cfg.price_decimals,
                );
                if let Some(reason) =
                    order_response_failure(&outcome, request_id.as_deref(), &mut runtime_state)
                {
                    session.order_response_health.mark_unhealthy(reason);
                }
            }
            for event in buffered_account {
                match apply_account_event(
                    event,
                    &mut AccountEventState {
                        ledger: &mut ledger,
                        stats: &mut stats,
                        projection: &mut session.projection,
                    },
                    &AccountEventContext {
                        symbol: &symbol,
                        run_order_prefix: &run_order_prefix,
                        mark: last_mark.unwrap_or(baseline_mark),
                        cycle,
                        output_format,
                    },
                ) {
                    Ok(outcome) => {
                        if outcome.requires_order_reconciliation {
                            cycle_invalidated_by_account = true;
                        }
                        let position = absorb_account_outcome(
                            outcome,
                            OutcomeSink {
                                total_fills: &mut total_fills,
                                balance_refresh_requested: &mut account_balance_refresh_requested,
                                inventory_exit_pending: &mut inventory_exit_pending,
                                notifier: &notifier,
                                position_alert_anchor: &mut position_alert_anchor,
                                expected_position: ledger.expected_position,
                                max_position: cfg.max_position,
                                inventory_exit_pct: args.inventory_exit_pct,
                                qty_tolerance,
                                symbol: &symbol,
                                cycle,
                                order_latency: Some(&mut session.order_latency),
                                latency_started: Some(session.latency_started),
                            },
                        )
                        .await;
                        if let Some(position) = position {
                            if (position - ledger.expected_position).abs() > qty_tolerance {
                                account_position_mismatch = Some(position);
                            } else {
                                account_position_mismatch = None;
                            }
                        }
                    }
                    Err(error) => {
                        runtime_state
                            .handle(MakerEvent::AccountStreamDisconnected(error.to_string()));
                        session
                            .account_stream_health
                            .mark_unhealthy(error.to_string());
                    }
                }
            }
        }
        if args.live {
            if let Some(exit) = accounting_invariant_exit(
                &notifier,
                &symbol,
                cycle,
                ledger.expected_position,
                stats.position(),
                qty_tolerance,
            )
            .await
            {
                break 'main exit;
            }
        }

        let cycle_result = if let Some(reconciliation) = reconciliation_error_for_cycle(
            ledger.expected_position,
            mismatch,
            account_position_mismatch.take(),
            cycle_invalidated_by_account,
        ) {
            Err(anyhow::Error::new(reconciliation))
        } else if let Some(cycle_result) = cycle_result {
            cycle_result
        } else {
            continue 'main;
        };

        if !matches!(
            runtime_state.pending_effect(),
            None | Some(MakerEffect::RunCycle(_))
        ) && cycle_result.is_ok()
        {
            // A fail-closed event invalidated the generation while cycle work
            // was running. Do not commit its counters/alerts; the queued
            // abort/cleanup effects are consumed by the recovery path.
            continue 'main;
        }

        match cycle_result {
            Ok((
                places,
                cancels,
                holds,
                fills,
                mark,
                src,
                market_fallback_reason,
                halted,
                exit_pending_after,
                balance,
            )) => {
                runtime_state.handle(MakerEvent::CycleCompleted(cycle_work_token));
                if !matches!(
                    runtime_state.next_effect(),
                    Some(MakerEffect::CommitCycle(token)) if token == cycle_work_token
                ) {
                    continue 'main;
                }
                consecutive_errors = 0;
                next_cycle_is_recovery = false;
                total_places += places;
                total_cancels += cancels;
                total_holds += holds;
                total_fills += fills;
                total_halted += halted as u64;
                last_mark = Some(mark);
                if halted != breaker_halted_before {
                    let (severity, event, message) = if halted {
                        (
                            "warning",
                            "entered",
                            "volatility breaker entered; maker quotes are being pulled",
                        )
                    } else {
                        (
                            "resolved",
                            "cleared",
                            "volatility breaker cleared; quoting may resume",
                        )
                    };
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "volatility_breaker",
                                severity,
                                event,
                                message,
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                }
                if !exit_pending_before && exit_pending_after {
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "warning",
                                event: "submitted",
                                message: "reduce-only inventory exit submitted",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                } else if exit_pending_before && !exit_pending_after {
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "resolved",
                                event: "confirmed",
                                message: "reduce-only inventory exit is no longer pending after ledger reconciliation",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: Some(ledger.expected_position),
                            },
                            false,
                        )
                    .await;
                }
                if !args.no_ws && last_src != Some(src) {
                    match src {
                        "ws" => eprintln!("✅ market feed: websocket live"),
                        _ => eprintln!(
                            "⚠️  market feed: REST fallback (reason={})",
                            market_fallback_reason.unwrap_or("ws_disabled")
                        ),
                    }
                    last_src = Some(src);
                }
                // Risk alerts: evaluate over the just-updated stats and
                // deliver any state changes (stderr always; webhook if set).
                let session_position = if args.live {
                    ledger.expected_position
                } else {
                    stats.position()
                };
                if alerts.enabled() {
                    let fired =
                        alerts.evaluate(&stats, session_position, mark, cfg.max_position, cycle);
                    for alert in fired {
                        // Await firing alerts so a breach raised on the final
                        // cycle before shutdown is not dropped with its task.
                        let await_delivery = alert.firing;
                        notifier.alert(&alert, &symbol, await_delivery).await;
                    }
                }
                // Account equity / available-margin floors. The snapshot is
                // only fetched in live mode, so these stay quiet in paper.
                if alerts.account_enabled() {
                    if let Some(balance) = balance.as_ref() {
                        let equity = balance.equity.parse::<f64>().ok();
                        let available = balance.cross_available.parse::<f64>().ok();
                        if let (Some(equity), Some(available)) = (equity, available) {
                            balance_floor_parse_warned = false;
                            let fired = alerts.evaluate_account(equity, available);
                            for alert in fired {
                                let await_delivery = alert.firing;
                                notifier.alert(&alert, &symbol, await_delivery).await;
                            }
                        } else if !balance_floor_parse_warned {
                            // An armed --alert-equity-below / --alert-margin-below
                            // must not go silently dark on unparseable balances.
                            balance_floor_parse_warned = true;
                            eprintln!(
                                "⚠️  equity/margin floor alerts skipped: unparseable balance fields (equity='{}', cross_available='{}')",
                                balance.equity, balance.cross_available
                            );
                        }
                    }
                }
                // Financial brake: a session loss breaching --stop-loss routes
                // through the fail-safe shutdown (freeze, cancel the maker
                // book, await the critical webhook, exit) — the same path the
                // other MakerExit variants use.
                if args.stop_loss > 0.0 {
                    let pnl = stats.pnl(session_position, mark);
                    if pnl <= -args.stop_loss {
                        emit_stop_loss_triggered(
                            output_format,
                            &symbol,
                            cycle,
                            pnl,
                            args.stop_loss,
                        );
                        notifier
                            .risk(
                                RiskNotice {
                                    kind: "stop_loss",
                                    severity: "critical",
                                    event: "triggered",
                                    message: &format!(
                                        "session PnL {pnl:+.2} breached stop-loss -{:.2}; shutting down",
                                        args.stop_loss
                                    ),
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: Some(ledger.expected_position),
                                    expected: Some(ledger.expected_position),
                                    observed: None,
                                },
                                true,
                            )
                            .await;
                        runtime_state.handle(MakerEvent::StopRequested(
                            RuntimeStopReason::StopLoss(format!(
                                "session PnL {pnl:+.2} <= -{:.2}",
                                args.stop_loss
                            )),
                        ));
                        break 'main take_stop_effect(
                            &mut runtime_state,
                            MakerExit::PositionReconciliation,
                        );
                    }
                }
            }
            Err(e) => {
                if e.downcast_ref::<ProjectionRegistryError>().is_some() {
                    let detail = format!("order-response correlation failed closed: {e}");
                    if let Some(session) = live_session.as_ref() {
                        session.order_response_health.mark_unhealthy(detail.clone());
                    }
                    runtime_state.handle(MakerEvent::OrderResponseDisconnected(detail));
                    continue 'main;
                }
                if let Some(mismatch) = e.downcast_ref::<PositionReconciliationError>() {
                    let reconciliation_cause = mismatch.cause.label();
                    // A mismatch is not a normal cycle error. Freeze quoting,
                    // empty the maker book, and give account-order callbacks
                    // plus REST settlement a bounded three-second window to
                    // converge before failing closed.
                    let recovery_token = match freeze_and_cleanup_for_recovery(
                        &mut RecoveryIo {
                            runtime_state: &mut runtime_state,
                            notifier: &notifier,
                            client: &client,
                            session: live_session.as_mut(),
                            resting: &mut resting,
                            inventory_exit_pending: &mut inventory_exit_pending,
                            consecutive_errors: &mut consecutive_errors,
                            next_cycle_is_recovery: &mut next_cycle_is_recovery,
                            symbol: &symbol,
                            cycle,
                            output_format,
                        },
                        FreezeSpec {
                            target: RecoveryTarget::PositionReconciliation,
                            trigger: MakerEvent::PositionMismatch,
                            cleanup_effect_stop: EffectFailureStop::CleanupFailure,
                            recovery_effect_stop: EffectFailureStop::PositionReconciliation,
                            cleanup_failure_prefix: String::new(),
                            cleanup_failed_exit: MakerExit::PositionReconciliation,
                            notice: RiskNotice {
                                kind: "position_reconciliation",
                                severity: "warning",
                                event: "frozen",
                                message: match &mismatch.cause {
                                    PositionReconciliationCause::CycleInvalidation => "account update invalidated active cycle; placements frozen and maker cleanup starting",
                                    PositionReconciliationCause::UnknownCurrentRunOrder => "unknown current-run order detected; placements frozen and maker cleanup starting",
                                    PositionReconciliationCause::AccountProjectionMismatch(_) => "account projection audit failed; placements frozen and maker cleanup starting",
                                    PositionReconciliationCause::PositionMismatch => "position mismatch detected; placements frozen and maker cleanup starting",
                                },
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: None,
                                expected: Some(mismatch.expected),
                                observed: Some(mismatch.observed),
                            },
                            frozen_note: Some(ReconciliationStateNote {
                                cause: reconciliation_cause,
                                expected: mismatch.expected,
                                observed: mismatch.observed,
                            }),
                            abort_account_stream_handle: false,
                            projection_reset: ProjectionReset::PreservePendingAcks,
                        },
                    )
                    .await
                    {
                        Ok(token) => token,
                        Err(exit) => break 'main exit,
                    };
                    // Genuine mismatch/projection anomalies share the same
                    // rolling budget as transport reconnects. A normal account
                    // event that invalidated in-flight cycle work is unmetered,
                    // but still follows the complete cleanup + REST reconcile +
                    // empty-book verification path below.
                    if let Some(admission) = reconciliation_recovery_admission(
                        mismatch,
                        &mut recovery_breaker,
                        recovery_clock_started.elapsed().as_secs(),
                    ) {
                        if !admission.is_admitted() {
                            break 'main recovery_failed_exit(
                                &mut runtime_state,
                                recovery_token,
                                format!(
                                    "{mismatch}; {}; refusing further live orders",
                                    recovery_circuit_detail(admission)
                                ),
                            );
                        }
                    }
                    let mut recovered = false;
                    let mut last_observed = mismatch.observed;
                    for delay in [500_u64, 1_000, 1_500] {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        if let Some(session) = live_session.as_mut() {
                            match apply_account_events(
                                &mut session.account_events,
                                &mut AccountEventState {
                                    ledger: &mut ledger,
                                    stats: &mut stats,
                                    projection: &mut session.projection,
                                },
                                &AccountEventContext {
                                    symbol: &symbol,
                                    run_order_prefix: &run_order_prefix,
                                    mark: last_mark.unwrap_or(baseline_mark),
                                    cycle,
                                    output_format,
                                },
                            ) {
                                Ok(outcome) => {
                                    if let Some(position) = absorb_account_outcome(
                                        outcome,
                                        OutcomeSink {
                                            total_fills: &mut total_fills,
                                            balance_refresh_requested:
                                                &mut account_balance_refresh_requested,
                                            inventory_exit_pending: &mut inventory_exit_pending,
                                            notifier: &notifier,
                                            position_alert_anchor: &mut position_alert_anchor,
                                            expected_position: ledger.expected_position,
                                            max_position: cfg.max_position,
                                            inventory_exit_pct: args.inventory_exit_pct,
                                            qty_tolerance,
                                            symbol: &symbol,
                                            cycle,
                                            order_latency: Some(&mut session.order_latency),
                                            latency_started: Some(session.latency_started),
                                        },
                                    )
                                    .await
                                    {
                                        last_observed = position;
                                    }
                                }
                                Err(error) => {
                                    session
                                        .account_stream_health
                                        .mark_unhealthy(error.to_string());
                                }
                            }
                        }
                        match probe_position_convergence(
                            &client,
                            ReconcileRequest {
                                symbol: &symbol,
                                session_started_at,
                                run_order_prefix: &run_order_prefix,
                                qty_tolerance,
                                mark: last_mark.unwrap_or(baseline_mark),
                            },
                            &mut ledger,
                            &mut stats,
                            &mut total_fills,
                            cycle,
                            output_format,
                        )
                        .await
                        {
                            ConvergenceProbe::Converged { observed } => {
                                last_observed = observed;
                                recovered = true;
                                break;
                            }
                            ConvergenceProbe::Pending { observed } => last_observed = observed,
                            ConvergenceProbe::SnapshotFailed(error) => {
                                emit_reconciliation_snapshot_error(
                                    output_format,
                                    &symbol,
                                    cycle,
                                    &error.to_string(),
                                )
                            }
                        }
                    }
                    if recovered {
                        // Requests already accepted by the venue can become
                        // visible after the initial freeze cleanup. Verify the
                        // maker book again at the end of the bounded recovery
                        // window before unfreezing placements.
                        if let Err(error) =
                            cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await
                        {
                            break 'main recovery_failed_exit(
                                &mut runtime_state,
                                recovery_token,
                                format!(
                                    "position reconciliation final maker-book verification failed: {error}"
                                ),
                            );
                        }
                        account_order_reconciliation_required = false;
                        resume_quoting_after_recovery(
                            &mut RecoveryIo {
                                runtime_state: &mut runtime_state,
                                notifier: &notifier,
                                client: &client,
                                session: live_session.as_mut(),
                                resting: &mut resting,
                                inventory_exit_pending: &mut inventory_exit_pending,
                                consecutive_errors: &mut consecutive_errors,
                                next_cycle_is_recovery: &mut next_cycle_is_recovery,
                                symbol: &symbol,
                                cycle,
                                output_format,
                            },
                            ResumeSpec {
                                recovery_token,
                                observed: last_observed,
                                projection_reset: ProjectionReset::PreservePendingAcks,
                                clear_resting: false,
                                reset_consecutive_errors: true,
                                recovered_note: Some(ReconciliationStateNote {
                                    cause: reconciliation_cause,
                                    expected: ledger.expected_position,
                                    observed: last_observed,
                                }),
                                notice: RiskNotice {
                                    kind: "position_reconciliation",
                                    severity: "resolved",
                                    event: "recovered",
                                    message: "position ledger recovered within the 3-second freeze window; quoting may resume from an empty maker book",
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: None,
                                    expected: Some(ledger.expected_position),
                                    observed: Some(last_observed),
                                },
                            },
                        )
                        .await;
                        continue;
                    }
                    emit_reconciliation_state(
                        output_format,
                        &symbol,
                        cycle,
                        "failed",
                        reconciliation_cause,
                        ledger.expected_position,
                        last_observed,
                    );
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "position_reconciliation",
                                severity: "critical",
                                event: "failed",
                                message: "position ledger remained inconsistent after the 3-second freeze window",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: None,
                                expected: Some(ledger.expected_position),
                                observed: Some(last_observed),
                            },
                            true,
                        )
                    .await;
                    runtime_state.handle(MakerEvent::RecoveryFailed {
                        token: recovery_token,
                        reason: format!(
                            "expected position {:+.8}, venue reported {:+.8} after 3s freeze",
                            ledger.expected_position, last_observed
                        ),
                    });
                    break 'main take_stop_effect(
                        &mut runtime_state,
                        MakerExit::PositionReconciliation,
                    );
                }
                if exit_pending_before {
                    let message = format!("inventory exit cycle failed: {e}");
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "warning",
                                event: "failed",
                                message: &message,
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                }
                runtime_state.handle(MakerEvent::CycleFailed {
                    token: cycle_work_token,
                    reason: e.to_string(),
                });
                consecutive_errors += 1;
                eprintln!("⚠️  maker cycle failed ({}/3): {}", consecutive_errors, e);
                if matches!(runtime_state.pending_effect(), Some(MakerEffect::Stop(_))) {
                    break take_stop_effect(&mut runtime_state, MakerExit::ConsecutiveErrors);
                }
            }
        }

        cycle += 1;

        if matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::RunCycle(_))
        ) {
            continue 'main;
        }
        if account_balance_refresh_requested && alerts.account_enabled() {
            // A balance event arrived while the just-finished cycle was doing
            // I/O. Skip the normal interval so the next cycle can fetch the
            // authoritative unified balance and evaluate account floors.
            continue 'main;
        }
        account_balance_refresh_requested = false;

        // Sleep until the next cycle, but wake early when a coherent market
        // update invalidates the prior decision: mark drift, a quote crossing
        // the new touch, or mark/mid divergence. The one-second floor keeps
        // this a bounded safety replan rather than a per-tick cancel loop.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(args.interval);
        let min_gap = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let request_deadline = live_session.as_ref().and_then(|session| {
                session
                    .order_request_deadlines
                    .next_deadline(ORDER_REQUEST_TIMEOUT)
            });
            let request_timeout = async {
                match request_deadline {
                    Some(deadline) => {
                        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await
                    }
                    None => std::future::pending().await,
                }
            };
            let update = async {
                match updates.as_mut() {
                    Some(rx) => rx.changed().await.is_ok(),
                    None => std::future::pending().await,
                }
            };
            let account_update = async {
                match live_session.as_mut() {
                    Some(session) => session.account_events.recv().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = ctrl_c_latched(&mut ctrl_c_rx) => {
                    runtime_state.handle(MakerEvent::StopRequested(RuntimeStopReason::CtrlC));
                    break 'main take_stop_effect(&mut runtime_state, MakerExit::PositionReconciliation);
                },
                _ = tokio::time::sleep_until(deadline) => break,
                _ = request_timeout => break,
                event = account_update => {
                    // The branch futures are dropped before select! handlers
                    // run, so the session can be re-borrowed here. An event
                    // only arrives when the live session exists.
                    match (event, live_session.as_mut()) {
                        (Some(event), Some(session)) => match apply_account_event(
                            event,
                            &mut AccountEventState {
                                ledger: &mut ledger,
                                stats: &mut stats,
                                projection: &mut session.projection,
                            },
                            &AccountEventContext {
                                symbol: &symbol,
                                run_order_prefix: &run_order_prefix,
                                mark: last_mark.unwrap_or(baseline_mark),
                                cycle,
                                output_format,
                            },
                        ) {
                            Ok(outcome) => {
                                account_order_reconciliation_required |=
                                    outcome.requires_order_reconciliation;
                                let position = absorb_account_outcome(
                                    outcome,
                                    OutcomeSink {
                                        total_fills: &mut total_fills,
                                        balance_refresh_requested: &mut account_balance_refresh_requested,
                                        inventory_exit_pending: &mut inventory_exit_pending,
                                        notifier: &notifier,
                                        position_alert_anchor: &mut position_alert_anchor,
                                        expected_position: ledger.expected_position,
                                        max_position: cfg.max_position,
                                        inventory_exit_pct: args.inventory_exit_pct,
                                        qty_tolerance,
                                        symbol: &symbol,
                                        cycle,
                                        order_latency: Some(&mut session.order_latency),
                                        latency_started: Some(session.latency_started),
                                    },
                                )
                                .await;
                                if let Some(position) = position.filter(|position| {
                                    (*position - ledger.expected_position).abs() > qty_tolerance
                                }) {
                                    account_position_mismatch = Some(position);
                                }
                                break;
                            }
                            Err(error) => {
                                session.account_stream_health.mark_unhealthy(error.to_string());
                                break;
                            }
                        },
                        (None, Some(session)) => {
                            session
                                .account_stream_health
                                .mark_unhealthy("authenticated account stream disconnected");
                            break;
                        }
                        (_, None) => break,
                    }
                }
                ok = update => {
                    if !ok {
                        // Feed task gone: fall back to plain interval waits.
                        updates = None;
                        continue;
                    }
                    if tokio::time::Instant::now() < min_gap {
                        continue;
                    }
                    let (Some(feed), Some(prev)) = (feed.as_ref(), last_mark) else {
                        continue;
                    };
                    let resting_for_replan = match live_session.as_ref() {
                        Some(session) => session.projection.resting_quotes(),
                        None => resting.clone(),
                    };
                    let requires_replan = {
                        let s = feed.read().await;
                        fresh_ws_snapshot(&s).is_some_and(|(mark, best_bid, best_ask)| {
                            market_update_requires_replan(
                                prev,
                                mark,
                                best_bid,
                                best_ask,
                                &resting_for_replan,
                                cfg.refresh_bps,
                                args.max_divergence_bps,
                            )
                        })
                    };
                    if requires_replan {
                        runtime_state.handle(MakerEvent::MarketChanged);
                        break; // early re-quote cycle
                    }
                }
            }
        }
    };

    // ---- Cleanup on ALL exit paths ----
    if let (Some(performance), Some(final_mark)) = (ledger.performance_mut(), last_mark) {
        let end_time_ms = performance_epoch_ms.saturating_add(
            i64::try_from(performance_started.elapsed().as_millis()).unwrap_or(i64::MAX),
        );
        if let Err(error) = performance.finish(end_time_ms) {
            eprintln!("⚠️ performance finalization unavailable: {error}");
        }
        match performance.summary(final_mark) {
            Ok(summary) => output::emit_performance_summary(output_format, &symbol, &summary),
            Err(error) => eprintln!("⚠️ performance summary unavailable: {error}"),
        }
    }
    let (account_stream_handle, order_response_handle) = match live_session {
        Some(mut session) => {
            let ended_ms =
                u64::try_from(session.latency_started.elapsed().as_millis()).unwrap_or(u64::MAX);
            if let Err(error) = session.order_latency.finish_process(ended_ms) {
                eprintln!("⚠️ order latency finalization unavailable: {error}");
            }
            output::emit_order_latency(output_format, &symbol, &session.order_latency);
            (
                Some(session.account_stream_handle),
                Some(session.order_response_handle),
            )
        }
        None => (None, None),
    };
    shutdown_report(ShutdownReport {
        live: args.live,
        output_format,
        symbol: &symbol,
        cfg: &cfg,
        client: &client,
        notifier: &notifier,
        ledger: &ledger,
        stats: &stats,
        breaker: &breaker,
        exit,
        cycle,
        total_places,
        total_cancels,
        total_holds,
        total_fills,
        total_halted,
        sim_position,
        last_mark,
        feed_handle,
        account_stream_handle,
        order_response_handle,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::pipeline::{OrderRequestKind, OrderRequestTimeoutPhase};
    use super::*;
    use standx_maker::{OrderObservation, ProjectionPendingCancel, ProjectionPendingPlace};
    use standx_sdk::account_stream::{OrderUpdate, PositionUpdate};
    use standx_sdk::order_response::OrderResponseHealth;

    #[test]
    fn request_timeout_detail_identifies_request_kind_and_missing_phase() {
        let detail = order_request_timeout_detail(&TimedOutOrderRequest {
            request_id: "request-7".to_string(),
            kind: OrderRequestKind::Cancel,
            phase: OrderRequestTimeoutPhase::AccountOrder,
            age: Duration::from_millis(10_250),
        });

        assert_eq!(
            detail,
            "order request lifecycle timed out after 10.250s: kind=cancel request_id=request-7 waiting_for=account_order; refusing further live orders"
        );
    }

    /// Pins the per-flow mapping from a missing/mismatched runtime effect to
    /// its stop reason: the order-response flow stops as OrderResponse even
    /// for cleanup failures, while the other flows carry the target through
    /// CleanupFailure or fall back to PositionReconciliation.
    #[test]
    fn effect_failure_stop_maps_each_flow_variant() {
        assert_eq!(
            effect_failure_stop(
                EffectFailureStop::CleanupFailure,
                RecoveryTarget::AccountStream,
                "boom".to_string(),
            ),
            RuntimeStopReason::CleanupFailure {
                target: RecoveryTarget::AccountStream,
                reason: "boom".to_string(),
            }
        );
        assert_eq!(
            effect_failure_stop(
                EffectFailureStop::CleanupFailure,
                RecoveryTarget::PositionReconciliation,
                "boom".to_string(),
            ),
            RuntimeStopReason::CleanupFailure {
                target: RecoveryTarget::PositionReconciliation,
                reason: "boom".to_string(),
            }
        );
        assert_eq!(
            effect_failure_stop(
                EffectFailureStop::OrderResponse,
                RecoveryTarget::OrderResponse,
                "boom".to_string(),
            ),
            RuntimeStopReason::OrderResponse("boom".to_string())
        );
        assert_eq!(
            effect_failure_stop(
                EffectFailureStop::PositionReconciliation,
                RecoveryTarget::AccountStream,
                "boom".to_string(),
            ),
            RuntimeStopReason::PositionReconciliation("boom".to_string())
        );
    }

    fn pending_place(request_id: &str) -> ProjectionPendingPlace {
        ProjectionPendingPlace {
            request_id: request_id.to_string(),
            client_order_id: format!("cl-{request_id}"),
            side: OrderSide::Buy,
            price: 100.0,
            qty: 1.0,
            level: 0,
            ref_center: 100.0,
            cycle: 1,
        }
    }

    fn account_balance() -> standx_sdk::models::Balance {
        standx_sdk::models::Balance {
            balance: "100".to_string(),
            cross_available: "90".to_string(),
            cross_balance: "100".to_string(),
            cross_margin: "0".to_string(),
            cross_upnl: "0".to_string(),
            equity: "100".to_string(),
            isolated_balance: "0".to_string(),
            isolated_upnl: "0".to_string(),
            locked: "0".to_string(),
            pnl_24h: "0".to_string(),
            pnl_freeze: "0".to_string(),
            upnl: "0".to_string(),
        }
    }

    fn projection_with_pending(request_ids: &[&str]) -> MakerAccountProjection {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        for request_id in request_ids {
            projection.apply(
                1,
                AccountProjectionEvent::PlaceSubmitted(pending_place(request_id)),
            );
        }
        projection
    }

    fn order_response(request_id: Option<&str>, code: i64) -> OrderResponse {
        OrderResponse {
            code,
            message: String::new(),
            request_id: request_id.map(str::to_string),
        }
    }

    #[test]
    fn maker_rest_client_is_isolated_from_order_response_session() {
        let client = new_maker_rest_client().expect("maker REST client is constructible");
        assert_eq!(client.session_id(), None);
    }

    fn position_update(symbol: &str, side: Option<OrderSide>, qty: &str) -> PositionUpdate {
        PositionUpdate {
            seq: 0,
            id: 0,
            symbol: symbol.to_string(),
            side,
            qty: qty.to_string(),
            entry_price: String::new(),
            realized_pnl: String::new(),
            status: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn runtime_effect_executor_orders_abort_cleanup_and_recovery() {
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let cycle_token = match runtime_state.next_effect() {
            Some(MakerEffect::RunCycle(token)) => token,
            effect => panic!("expected cycle effect, got {effect:?}"),
        };

        runtime_state.handle(MakerEvent::PositionMismatch);
        let cleanup =
            take_cleanup_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("abort must be drained before cleanup");
        runtime_state.handle(MakerEvent::CycleCompleted(cycle_token));
        assert!(runtime_state.pending_effect().is_none());

        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery =
            take_recovery_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("cleanup completion must schedule recovery");
        runtime_state.handle(MakerEvent::RecoverySucceeded(recovery));
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));
    }

    #[test]
    fn ws_balance_request_schedules_immediate_authoritative_refresh_for_account_alerts() {
        let now = std::time::Instant::now();
        let mut poll = LiveAccountPollState::new(account_balance(), now);
        let mut requested = true;

        assert!(!poll.balance_refresh_due(now));
        assert!(schedule_account_balance_refresh(
            &mut requested,
            true,
            &mut poll,
            now,
        ));
        assert!(!requested);
        assert!(poll.balance_refresh_due(now));

        let mut disabled_request = true;
        let later = now + Duration::from_secs(1);
        poll.record_balance_refresh(account_balance(), later);
        assert!(!schedule_account_balance_refresh(
            &mut disabled_request,
            false,
            &mut poll,
            later,
        ));
        assert!(!disabled_request);
        assert!(!poll.balance_refresh_due(later));
    }

    #[test]
    fn runtime_recovery_failure_is_the_stop_source_of_truth() {
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = runtime_state.next_effect();
        runtime_state.handle(MakerEvent::OrderResponseDisconnected("closed".to_string()));
        let cleanup = take_cleanup_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
            .expect("cleanup effect");
        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery = take_recovery_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
            .expect("recovery effect");
        let exit = recovery_failed_exit(
            &mut runtime_state,
            recovery,
            "residual maker orders".to_string(),
        );
        assert!(
            matches!(exit, MakerExit::OrderResponse(reason) if reason == "residual maker orders")
        );
    }

    #[test]
    fn apply_order_response_keeps_accepted_placement() {
        let mut projection = projection_with_pending(&["req-1"]);
        let matched = apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap();
        assert!(matched);
        assert_eq!(
            projection.pending_places().len(),
            1,
            "accepted placement stays pending"
        );
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn order_response_correlation_failed_only_on_uncorrelated_request_ids() {
        // A matched ack is never a correlation failure, even while the runtime
        // is frozen for another reason.
        assert!(!order_response_correlation_failed(true, Some("req-1")));
        // A response whose request_id matches no pending request fails closed.
        assert!(order_response_correlation_failed(false, Some("req-1")));
        // A response without a request_id cannot be correlated or escalated.
        assert!(!order_response_correlation_failed(false, None));
    }

    #[test]
    fn account_invalidation_with_matched_buffered_ack_reconciles_without_order_response_stop() {
        // Reproduces the shutdown that a plan-affecting account event (e.g. a
        // fill) used to trigger when the cycle had already buffered one of its
        // own order acks: the freeze targets position reconciliation, but the
        // buffered ack was wrongly read as an order-response correlation
        // failure, flipping a healthy stream unhealthy and colliding with the
        // queued cleanup target.
        let mut projection = projection_with_pending(&["req-1"]);

        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let cycle_token = match runtime_state.next_effect() {
            Some(MakerEffect::RunCycle(token)) => token,
            effect => panic!("expected cycle effect, got {effect:?}"),
        };

        // An invalidating account event freezes the in-flight cycle and queues
        // AbortInFlight + Cleanup { PositionReconciliation }.
        runtime_state.handle(MakerEvent::CycleInvalidated {
            reason: "account state changed during maker cycle".to_string(),
        });

        // The cycle's own placement ack was buffered before the freeze and is
        // now drained. It correlates with the pending request, so it matches.
        let health = OrderResponseHealth::default();
        let response = order_response(Some("req-1"), 0);
        let request_id = response.request_id.clone();
        let matched = apply_order_response(
            response,
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap();
        assert!(matched, "buffered ack correlates with the pending request");
        if order_response_correlation_failed(matched, request_id.as_deref()) {
            health.mark_unhealthy("order-response correlation failed closed");
        }

        // A matched ack must leave the order-response stream healthy; otherwise
        // the top-of-loop health check would demand an OrderResponse cleanup.
        assert!(
            health.is_healthy(),
            "a matched ack must not flip the order-response stream unhealthy"
        );

        // The queued cleanup targets position reconciliation, so the maker
        // cleans up and can recover instead of stopping.
        take_cleanup_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
            .expect("invalidation must drive a position-reconciliation cleanup, not a stop");

        // Stale completion of the aborted cycle is ignored; the maker stays
        // frozen awaiting recovery rather than resuming on stale work.
        runtime_state.handle(MakerEvent::CycleCompleted(cycle_token));
        assert!(runtime_state.pending_effect().is_none());
    }

    #[test]
    fn order_response_cleanup_drain_rejects_position_reconciliation_target() {
        // Regression witness for the collision the fix removes: had a buffered
        // response been treated as an order-response fault while the runtime
        // was frozen for position reconciliation, the top-of-loop
        // order-response recovery would drain the queued cleanup with the wrong
        // target and fail closed into a stop.
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = runtime_state.next_effect();
        runtime_state.handle(MakerEvent::CycleInvalidated {
            reason: "account state changed during maker cycle".to_string(),
        });
        let error = take_cleanup_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
            .expect_err("position-reconciliation cleanup must not satisfy an order-response drain");
        assert!(error.to_string().contains("expected OrderResponse cleanup"));
    }

    #[test]
    fn apply_order_response_drops_rejected_placement() {
        let mut projection = projection_with_pending(&["req-1"]);
        let matched = apply_order_response(
            order_response(Some("req-1"), 1),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap();
        assert!(matched);
        assert!(
            projection.pending_places().is_empty(),
            "rejected placement is removed"
        );
    }

    #[test]
    fn apply_order_response_matches_cancel_acknowledgement() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        assert!(apply_order_response(
            order_response(Some("cancel-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        assert!(projection.pending_cancels().is_empty());
    }

    #[test]
    fn duplicate_place_ack_matches_completed_request_after_cleanup() {
        let mut projection = projection_with_pending(&["req-1"]);
        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        projection.clear_orders_and_pending();

        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            2,
            2,
        )
        .unwrap());
    }

    #[test]
    fn delayed_account_order_and_replayed_ack_survive_account_reconnect() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
                request_id: "req-1".to_string(),
                client_order_id: "sxmk-test-q00000001b0".to_string(),
                side: OrderSide::Buy,
                price: 100.0,
                qty: 1.0,
                level: 0,
                ref_center: 100.0,
                cycle: 1,
            }),
        );
        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        projection.apply(1, AccountProjectionEvent::AdvanceCycle { cycle: 4 });
        projection.reset_after_cleanup_preserving_pending_acks(2, 0.0);

        let outcome = projection.apply(
            2,
            AccountProjectionEvent::OrderObserved(OrderObservation {
                order_id: 7,
                client_order_id: Some("sxmk-test-q00000001b0".to_string()),
                side: OrderSide::Buy,
                price: 100.0,
                open_qty: 1.0,
                terminal: false,
            }),
        );
        assert!(!outcome.unknown_current_run_order);
        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            4,
            2,
        )
        .unwrap());
    }

    #[test]
    fn duplicate_place_rejection_matches_completed_request_after_cleanup() {
        let mut projection = projection_with_pending(&["req-1"]);
        assert!(apply_order_response(
            order_response(Some("req-1"), 400),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        projection.clear_orders_and_pending();

        assert!(apply_order_response(
            order_response(Some("req-1"), 400),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            2,
            2,
        )
        .unwrap());
    }

    #[test]
    fn duplicate_cancel_ack_matches_completed_request_after_cleanup() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );
        assert!(apply_order_response(
            order_response(Some("cancel-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        projection.clear_orders_and_pending();

        assert!(apply_order_response(
            order_response(Some("cancel-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            2,
            2,
        )
        .unwrap());
    }

    #[test]
    fn contradictory_replay_for_completed_request_remains_fail_closed() {
        let mut projection = projection_with_pending(&["req-1"]);
        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());

        assert!(!apply_order_response(
            order_response(Some("req-1"), 400),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            2,
            2,
        )
        .unwrap());
    }

    #[test]
    fn apply_order_response_fails_closed_on_rejected_cancel_acknowledgement() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        assert_eq!(
            apply_order_response(
                order_response(Some("cancel-1"), 400),
                &mut projection,
                OutputFormat::Quiet,
                "BTC-USD",
                1,
                2,
            ),
            Err(CancelRejection {
                request_id: "cancel-1".to_string(),
                code: 400,
                message: String::new(),
            })
        );
        assert_eq!(projection.pending_cancels().len(), 1);
        assert_eq!(projection.pending_request_count(), 1);
    }

    #[test]
    fn apply_order_response_matches_late_ack_after_terminal_account_order() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
                request_id: "req-1".to_string(),
                client_order_id: "sxmk-test-q00000001b0".to_string(),
                side: OrderSide::Buy,
                price: 100.0,
                qty: 1.0,
                level: 0,
                ref_center: 100.0,
                cycle: 1,
            }),
        );
        projection.apply(
            1,
            AccountProjectionEvent::OrderObserved(OrderObservation {
                order_id: 7,
                client_order_id: Some("sxmk-test-q00000001b0".to_string()),
                side: OrderSide::Buy,
                price: 100.0,
                open_qty: 0.0,
                terminal: true,
            }),
        );
        assert!(projection.pending_places().is_empty());
        assert_eq!(projection.pending_request_count(), 1);

        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_response_reports_unmatched_ids() {
        let mut projection = projection_with_pending(&["req-1"]);
        assert!(!apply_order_response(
            order_response(Some("other"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        assert!(!apply_order_response(
            order_response(None, 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap());
        assert_eq!(projection.pending_places().len(), 1);
    }

    #[test]
    fn apply_order_responses_matched_acks_clear_request_registry() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut projection = projection_with_pending(&["req-1", "req-2"]);
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));

        // Benign matched acknowledgements for placements we are tracking.
        tx.try_send(order_response(Some("req-1"), 0)).unwrap();
        tx.try_send(order_response(Some("req-2"), 0)).unwrap();

        apply_order_responses(
            &mut rx,
            &mut projection,
            &mut runtime_state,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .expect("benign matched acks must not fail closed");

        assert!(runtime_state.pending_effect().is_none());
        // Accepted placements remain pending; the matched arm keeps them.
        assert_eq!(projection.pending_places().len(), 2);
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_responses_unknown_request_fails_closed() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut projection = projection_with_pending(&[]);
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));

        tx.try_send(order_response(Some("req-1"), 0)).unwrap();
        let error = apply_order_responses(
            &mut rx,
            &mut projection,
            &mut runtime_state,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("correlation failed closed"));
        assert!(error.to_string().contains("request_id=req-1"));
        assert!(matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::AbortInFlight(_))
        ));
    }

    #[test]
    fn apply_order_responses_rejected_cancel_fails_closed() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));

        tx.try_send(OrderResponse {
            code: 400,
            message: "cancel rejected".to_string(),
            request_id: Some("cancel-1".to_string()),
        })
        .unwrap();
        let error = apply_order_responses(
            &mut rx,
            &mut projection,
            &mut runtime_state,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap_err();

        assert!(error.to_string().contains("cancel rejected"));
        assert!(matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::AbortInFlight(_))
        ));
        assert_eq!(projection.pending_cancels().len(), 1);
    }

    #[test]
    fn plan_affecting_account_events_invalidate_cycle_work() {
        assert!(account_event_invalidates_cycle(&AccountEvent::Position(
            position_update("BTC-USD", Some(OrderSide::Buy), "0.5")
        )));
        assert!(account_event_invalidates_cycle(&AccountEvent::Error {
            reason: "bad payload".to_string(),
        }));
        assert!(!account_event_invalidates_cycle(&AccountEvent::Order(
            OrderUpdate {
                seq: 1,
                order_id: 7,
                cl_ord_id: Some("sxmk-test-q00000001b0".to_string()),
                symbol: "BTC-USD".to_string(),
                side: OrderSide::Buy,
                qty: "1".to_string(),
                fill_qty: "0".to_string(),
                fill_avg_price: "0".to_string(),
                price: "100".to_string(),
                status: standx_sdk::models::OrderStatus::Open,
                reduce_only: false,
                updated_at: String::new(),
            }
        )));
        assert!(!account_event_invalidates_cycle(&AccountEvent::Connected {
            epoch: 1,
        }));
    }

    #[test]
    fn account_cycle_invalidation_routes_through_cleanup_without_a_position_gap() {
        let reconciliation = reconciliation_error_for_cycle(0.2, None, None, true)
            .expect("an invalidated cycle must enter reconciliation cleanup");
        assert_eq!(reconciliation.expected, 0.2);
        assert_eq!(reconciliation.observed, 0.2);
        assert_eq!(reconciliation.cause.label(), "cycle_invalidation");

        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let cycle_token = match runtime_state.next_effect() {
            Some(MakerEffect::RunCycle(token)) => token,
            effect => panic!("expected cycle effect, got {effect:?}"),
        };
        runtime_state.handle(MakerEvent::CycleInvalidated {
            reason: "account state changed during maker cycle".to_string(),
        });
        // `PositionMismatch` is deliberately a no-op while frozen. The
        // pending abort is consumed by the cleanup executor instead of being
        // misread as an unexpected effect before the next cycle.
        runtime_state.handle(MakerEvent::PositionMismatch);
        let cleanup =
            take_cleanup_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("invalidated cycle must drain AbortInFlight before cleanup");
        runtime_state.handle(MakerEvent::CycleCompleted(cycle_token));
        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery =
            take_recovery_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("cleanup must lead to recovery");
        runtime_state.handle(MakerEvent::RecoverySucceeded(recovery));
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));
    }

    #[test]
    fn normal_cycle_invalidations_do_not_consume_recovery_incident_budget() {
        let invalidation = PositionReconciliationError::cycle_invalidation(0.0);
        let mismatch = PositionReconciliationError::position_mismatch(0.0, 0.2);
        let mut breaker = maker::RecoveryCircuitBreaker::new(1, 3_600);

        for now in [10, 20, 30, 40] {
            assert!(reconciliation_recovery_admission(&invalidation, &mut breaker, now).is_none());
        }
        assert!(
            reconciliation_recovery_admission(&mismatch, &mut breaker, 50)
                .expect("a true mismatch must be metered")
                .is_admitted()
        );
        assert!(
            !reconciliation_recovery_admission(&mismatch, &mut breaker, 60)
                .expect("a true mismatch must be metered")
                .is_admitted()
        );
    }

    #[test]
    fn touch_or_divergence_can_request_an_early_replan_without_mark_drift() {
        let quote = RestingQuote {
            order_id: Some("7".to_string()),
            side: OrderSide::Buy,
            level: 0,
            price: 99.95,
            qty: 0.1,
            ref_center: 100.0,
            placed_at_cycle: 1,
        };
        assert!(market_update_requires_replan(
            100.0,
            100.0,
            Some(99.90),
            Some(99.95),
            std::slice::from_ref(&quote),
            3.0,
            25.0,
        ));
        assert!(market_update_requires_replan(
            100.0,
            100.0,
            Some(90.0),
            Some(90.1),
            &[quote],
            3.0,
            25.0,
        ));
        assert!(!market_update_requires_replan(
            100.0,
            100.0,
            Some(99.90),
            Some(99.96),
            &[],
            3.0,
            25.0,
        ));
    }

    fn drain_positions(events: Vec<AccountEvent>) -> AccountEventOutcome {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        for event in events {
            tx.try_send(event).unwrap();
        }
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        let mut state = AccountEventState {
            ledger: &mut ledger,
            stats: &mut stats,
            projection: &mut projection,
        };
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        apply_account_events(&mut rx, &mut state, &context).expect("benign events drain cleanly")
    }

    #[test]
    fn apply_account_events_records_position_mismatch_with_sign() {
        let buy = drain_positions(vec![AccountEvent::Position(position_update(
            "BTC-USD",
            Some(OrderSide::Buy),
            "0.5",
        ))]);
        assert_eq!(buy.latest_position, Some(0.5));

        let sell = drain_positions(vec![AccountEvent::Position(position_update(
            "BTC-USD",
            Some(OrderSide::Sell),
            "0.5",
        ))]);
        assert_eq!(
            sell.latest_position,
            Some(-0.5),
            "sell position is negative"
        );
    }

    #[test]
    fn apply_account_events_applies_buffered_events_in_order() {
        // The last position update in the buffer wins; benign Connected /
        // Balance events are drained without contributing fills.
        let outcome = drain_positions(vec![
            AccountEvent::Connected { epoch: 1 },
            AccountEvent::Position(position_update("BTC-USD", Some(OrderSide::Buy), "0.2")),
            AccountEvent::Balance(standx_sdk::account_stream::BalanceUpdate {
                seq: 1,
                account_type: "perps".to_string(),
                token: "DUSD".to_string(),
                free: "1".to_string(),
                total: "1".to_string(),
                locked: "0".to_string(),
                occupied: "0".to_string(),
                updated_at: "2026-07-14T00:00:00Z".to_string(),
            }),
            AccountEvent::Position(position_update("BTC-USD", Some(OrderSide::Sell), "0.9")),
        ]);
        assert_eq!(outcome.fills, 0);
        assert_eq!(
            outcome.latest_position,
            Some(-0.9),
            "latest position reflects last update"
        );
    }

    #[test]
    fn balance_event_updates_raw_projection_without_touching_fill_accounting() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        let outcome = {
            let mut state = AccountEventState {
                ledger: &mut ledger,
                stats: &mut stats,
                projection: &mut projection,
            };
            apply_account_event(
                AccountEvent::Balance(standx_sdk::account_stream::BalanceUpdate {
                    seq: 1,
                    account_type: "perps".to_string(),
                    token: "DUSD".to_string(),
                    free: "90".to_string(),
                    total: "100".to_string(),
                    locked: "0".to_string(),
                    occupied: "10".to_string(),
                    updated_at: "2026-07-14T00:00:00Z".to_string(),
                }),
                &mut state,
                &context,
            )
            .unwrap()
        };
        assert_eq!(outcome.fills, 0);
        // A balance event requests a REST refresh but does not mutate the
        // projection (raw wallet fields are not projected).
        assert!(outcome.balance_changed);
        assert_eq!(stats.fills(), 0);
    }

    #[test]
    fn uncorrelated_current_run_order_requires_reconciliation_without_stream_failure() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
        let mut state = AccountEventState {
            ledger: &mut ledger,
            stats: &mut stats,
            projection: &mut projection,
        };
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 2,
            output_format: OutputFormat::Quiet,
        };
        let update = OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-test-q00000001b0".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            qty: "0.2".to_string(),
            fill_qty: "0".to_string(),
            fill_avg_price: "0".to_string(),
            price: "100".to_string(),
            status: standx_sdk::models::OrderStatus::Open,
            reduce_only: false,
            updated_at: "2026-07-15T00:00:00Z".to_string(),
        };

        let outcome = apply_account_event(AccountEvent::Order(update), &mut state, &context)
            .expect("a late current-run order is a reconciliation trigger, not stream failure");

        assert!(outcome.requires_order_reconciliation);
        assert_eq!(outcome.fills, 0);
    }

    #[test]
    fn typed_trade_event_is_booked_once_after_order_ownership() {
        let order = standx_sdk::account_stream::OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-test-q00000001b0".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            qty: "0.2".to_string(),
            fill_qty: "0.2".to_string(),
            fill_avg_price: "100".to_string(),
            price: "100".to_string(),
            status: standx_sdk::models::OrderStatus::Filled,
            reduce_only: false,
            updated_at: "2026-07-14T00:00:00Z".to_string(),
        };
        let trade = standx_sdk::account_stream::TradeUpdate {
            seq: 2,
            trade_id: 11,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            price: "100".to_string(),
            qty: "0.2".to_string(),
            trade_ts: "2026-07-14T00:00:00Z".to_string(),
        };

        let outcome = drain_positions(vec![
            AccountEvent::Order(order),
            AccountEvent::Trade(trade.clone()),
            AccountEvent::Trade(trade),
        ]);
        assert_eq!(outcome.fills, 1);
        assert_eq!(outcome.latest_position, None);
    }

    #[test]
    fn apply_account_events_ignores_other_symbols() {
        let outcome = drain_positions(vec![AccountEvent::Position(position_update(
            "ETH-USD",
            Some(OrderSide::Buy),
            "1.0",
        ))]);
        assert_eq!(outcome.fills, 0);
        assert_eq!(
            outcome.latest_position, None,
            "position updates for other symbols are ignored"
        );
    }

    #[test]
    fn stable_trade_reports_current_run_inventory_exit_once() {
        let mut ledger = MakerLedger::new(0.2);
        let mut stats = MakerStats::with_inventory_baseline(0.2, 100.0);
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.2, 0.005, 0.00005);
        let mut state = AccountEventState {
            ledger: &mut ledger,
            stats: &mut stats,
            projection: &mut projection,
        };
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        let update = OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-test-x00000001".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Sell,
            qty: "0.2".to_string(),
            fill_qty: "0.2".to_string(),
            fill_avg_price: "100".to_string(),
            price: "100".to_string(),
            status: standx_sdk::models::OrderStatus::Filled,
            reduce_only: true,
            updated_at: "2026-07-14T00:00:00Z".to_string(),
        };

        let order = apply_account_event(AccountEvent::Order(update), &mut state, &context)
            .expect("exit order is valid");
        assert_eq!(order.fills, 0);
        assert!(!order.exit_fill_observed);

        let trade = standx_sdk::account_stream::TradeUpdate {
            seq: 2,
            trade_id: 11,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Sell,
            price: "100".to_string(),
            qty: "0.2".to_string(),
            trade_ts: "2026-07-14T00:00:00Z".to_string(),
        };
        let first = apply_account_event(AccountEvent::Trade(trade.clone()), &mut state, &context)
            .expect("exit trade is valid");
        assert_eq!(first.fills, 1);
        assert!(first.exit_fill_observed);

        let duplicate = apply_account_event(AccountEvent::Trade(trade), &mut state, &context)
            .expect("duplicate exit fill is valid");
        assert_eq!(duplicate.fills, 0);
        assert!(!duplicate.exit_fill_observed);
    }

    #[test]
    fn accounting_position_mismatch_respects_half_tick_tolerance() {
        let tolerance = 0.0005;
        assert!(!accounting_position_mismatch(0.2, 0.20049, tolerance));
        assert!(accounting_position_mismatch(0.2, 0.20051, tolerance));
        assert!(!accounting_position_mismatch(-0.2, -0.20049, tolerance));
        assert!(accounting_position_mismatch(-0.2, -0.20051, tolerance));
    }

    #[test]
    fn accounting_position_mismatch_fails_closed_on_non_finite() {
        let tolerance = 0.0005;
        assert!(accounting_position_mismatch(f64::NAN, 0.2, tolerance));
        assert!(accounting_position_mismatch(0.2, f64::NAN, tolerance));
        assert!(accounting_position_mismatch(f64::INFINITY, 0.2, tolerance));
    }

    #[tokio::test]
    async fn accounting_invariant_mismatch_becomes_fail_safe_exit() {
        let notifier = MakerNotifier::new(
            OutputFormat::Quiet,
            None,
            crate::cli::AlertWebhookFormat::Raw,
        );

        assert!(
            accounting_invariant_exit(&notifier, "XAG-USD", 1396, 0.0, -0.2, 0.0005,)
                .await
                .is_some_and(|exit| matches!(exit, MakerExit::AccountingInvariant(_)))
        );
        assert!(
            accounting_invariant_exit(&notifier, "XAG-USD", 1396, 0.0, 0.00049, 0.0005,)
                .await
                .is_none()
        );
    }

    // ---- Fault-injection conformance tests for the shared recovery helpers ----

    struct JwtGuard {
        original: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl JwtGuard {
        fn set() -> Self {
            // Share the crate-wide env lock so this STANDX_JWT mutation cannot
            // race env reads in other modules' tests. See crate::TEST_ENV_LOCK.
            let lock = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let original = std::env::var("STANDX_JWT").ok();
            std::env::set_var("STANDX_JWT", "runtime-test-jwt");
            Self {
                original,
                _lock: lock,
            }
        }
    }

    impl Drop for JwtGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var("STANDX_JWT", value),
                None => std::env::remove_var("STANDX_JWT"),
            }
        }
    }

    fn quiet_notifier() -> MakerNotifier {
        MakerNotifier::new(
            OutputFormat::Quiet,
            None,
            crate::cli::AlertWebhookFormat::Raw,
        )
    }

    fn resting_quote() -> RestingQuote {
        RestingQuote {
            order_id: None,
            side: OrderSide::Buy,
            level: 0,
            price: 100.0,
            qty: 0.001,
            ref_center: 100.0,
            placed_at_cycle: 1,
        }
    }

    fn warning_notice(kind: &'static str) -> RiskNotice<'static> {
        RiskNotice {
            kind,
            severity: "warning",
            event: "disconnected_frozen",
            message: "test freeze",
            symbol: "BTC-USD",
            cycle: 7,
            position_before: None,
            position_after: None,
            expected: Some(0.0),
            observed: None,
        }
    }

    fn order_response_freeze_spec() -> FreezeSpec<'static> {
        FreezeSpec {
            target: RecoveryTarget::OrderResponse,
            trigger: MakerEvent::OrderResponseDisconnected("stream closed".to_string()),
            cleanup_effect_stop: EffectFailureStop::OrderResponse,
            recovery_effect_stop: EffectFailureStop::OrderResponse,
            cleanup_failure_prefix: "order-response ".to_string(),
            cleanup_failed_exit: MakerExit::OrderResponse,
            notice: warning_notice("order_response"),
            frozen_note: None,
            abort_account_stream_handle: false,
            projection_reset: ProjectionReset::DropPendingRequests,
        }
    }

    /// Invariant: the freeze preamble empties the maker book on the venue
    /// (cancelling only maker-owned orders), clears local book state, and
    /// hands back a recovery token from which quoting can resume.
    #[tokio::test]
    async fn freeze_preamble_empties_the_maker_book_and_hands_back_recovery() {
        use mockito::{Matcher, Server};
        let _jwt = JwtGuard::set();
        let mut server = Server::new_async().await;
        let open_before = server
            .mock("GET", "/api/query_open_orders")
            .match_query(Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"code":0,"message":"ok","result":[
                    {"id":"42","cl_ord_id":"sxmk-freeze-buy","symbol":"BTC-USD","side":"buy","order_type":"limit","qty":"0.001","fill_qty":"0","price":"63000","status":"open","created_at":"2026-07-10T00:00:00Z","updated_at":"2026-07-10T00:00:00Z"},
                    {"id":"99","cl_ord_id":"manual-order","symbol":"BTC-USD","side":"sell","order_type":"limit","qty":"0.001","fill_qty":"0","price":"65000","status":"open","created_at":"2026-07-10T00:00:00Z","updated_at":"2026-07-10T00:00:00Z"}
                ]}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let cancel = server
            .mock("POST", "/api/cancel_orders")
            .match_body(Matcher::Json(serde_json::json!({ "order_id_list": [42] })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":0,"message":"accepted"}"#)
            .expect(1)
            .create_async()
            .await;
        let open_after = server
            .mock("GET", "/api/query_open_orders")
            .match_query(Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"code":0,"message":"ok","result":[
                    {"id":"99","cl_ord_id":"manual-order","symbol":"BTC-USD","side":"sell","order_type":"limit","qty":"0.001","fill_qty":"0","price":"65000","status":"open","created_at":"2026-07-10T00:00:00Z","updated_at":"2026-07-10T00:00:00Z"}
                ]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let notifier = quiet_notifier();
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));
        let mut resting = vec![resting_quote()];
        let mut inventory_exit_pending = true;
        let mut consecutive_errors = 2;
        let mut next_cycle_is_recovery = false;

        let recovery_token = freeze_and_cleanup_for_recovery(
            &mut RecoveryIo {
                runtime_state: &mut runtime_state,
                notifier: &notifier,
                client: &client,
                session: None,
                resting: &mut resting,
                inventory_exit_pending: &mut inventory_exit_pending,
                consecutive_errors: &mut consecutive_errors,
                next_cycle_is_recovery: &mut next_cycle_is_recovery,
                symbol: "BTC-USD",
                cycle: 7,
                output_format: OutputFormat::Quiet,
            },
            order_response_freeze_spec(),
        )
        .await
        .expect("freeze preamble must hand back a recovery token");

        assert!(resting.is_empty(), "local book must be cleared");
        assert!(!inventory_exit_pending);
        assert!(
            runtime_state.pending_effect().is_none(),
            "no stale effects may remain after the preamble"
        );
        open_before.assert_async().await;
        cancel.assert_async().await;
        open_after.assert_async().await;

        // Recovery success must resume quoting with a fresh cycle.
        runtime_state.handle(MakerEvent::RecoverySucceeded(recovery_token));
        assert!(matches!(
            runtime_state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));
    }

    /// Invariant: when the venue book cannot be emptied, the preamble stops
    /// the runtime with the flow's exit and its exact historical wording.
    #[tokio::test]
    async fn freeze_preamble_cleanup_failure_stops_with_the_flow_exit() {
        use mockito::{Matcher, Server};
        let _jwt = JwtGuard::set();
        let mut server = Server::new_async().await;
        let open_orders = server
            .mock("GET", "/api/query_open_orders")
            .match_query(Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(500)
            .with_body("venue unavailable")
            .expect_at_least(1)
            .create_async()
            .await;

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let notifier = quiet_notifier();
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = runtime_state.next_effect();
        let mut resting = vec![resting_quote()];
        let mut inventory_exit_pending = false;
        let mut consecutive_errors = 0;
        let mut next_cycle_is_recovery = false;

        let exit = freeze_and_cleanup_for_recovery(
            &mut RecoveryIo {
                runtime_state: &mut runtime_state,
                notifier: &notifier,
                client: &client,
                session: None,
                resting: &mut resting,
                inventory_exit_pending: &mut inventory_exit_pending,
                consecutive_errors: &mut consecutive_errors,
                next_cycle_is_recovery: &mut next_cycle_is_recovery,
                symbol: "BTC-USD",
                cycle: 7,
                output_format: OutputFormat::Quiet,
            },
            order_response_freeze_spec(),
        )
        .await
        .expect_err("cleanup failure must stop the runtime");

        match exit {
            MakerExit::OrderResponse(reason) => {
                assert!(
                    reason.contains("order-response freeze cleanup failed:"),
                    "cleanup-failure wording drifted: {reason}"
                );
            }
            other => panic!(
                "order-response cleanup failure must exit as OrderResponse, got {:?}",
                other.lifecycle_reason()
            ),
        }
        // The runtime is stopping: no further work may be scheduled.
        runtime_state.handle(MakerEvent::Timer);
        assert!(runtime_state.pending_effect().is_none());
        open_orders.assert_async().await;
    }

    /// Invariant: if the runtime cannot enter the freeze (it is already
    /// stopping), the preamble fails closed instead of proceeding to cleanup.
    #[tokio::test]
    async fn freeze_preamble_fails_closed_when_runtime_cannot_freeze() {
        let client = StandXClient::new().unwrap();
        let notifier = quiet_notifier();
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = runtime_state.next_effect();
        runtime_state.handle(MakerEvent::StopRequested(RuntimeStopReason::CtrlC));
        while runtime_state.next_effect().is_some() {}
        let mut resting = vec![resting_quote()];
        let mut inventory_exit_pending = false;
        let mut consecutive_errors = 0;
        let mut next_cycle_is_recovery = false;

        let exit = freeze_and_cleanup_for_recovery(
            &mut RecoveryIo {
                runtime_state: &mut runtime_state,
                notifier: &notifier,
                client: &client,
                session: None,
                resting: &mut resting,
                inventory_exit_pending: &mut inventory_exit_pending,
                consecutive_errors: &mut consecutive_errors,
                next_cycle_is_recovery: &mut next_cycle_is_recovery,
                symbol: "BTC-USD",
                cycle: 7,
                output_format: OutputFormat::Quiet,
            },
            order_response_freeze_spec(),
        )
        .await
        .expect_err("a stopping runtime must not begin cleanup");
        assert!(matches!(exit, MakerExit::PositionReconciliation(_)));
        assert!(
            !resting.is_empty(),
            "no cleanup may run when the freeze was rejected"
        );
    }

    /// Invariant: the resume tail restores quoting state (flags, error
    /// streak, paper book) and schedules the next cycle via the runtime.
    #[tokio::test]
    async fn resume_tail_restores_quoting_state_and_schedules_a_cycle() {
        let client = StandXClient::new().unwrap();
        let notifier = quiet_notifier();
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = runtime_state.next_effect();
        runtime_state.handle(MakerEvent::PositionMismatch);
        let _ = runtime_state.next_effect(); // AbortInFlight
        let cleanup = match runtime_state.next_effect() {
            Some(MakerEffect::Cleanup { token, .. }) => token,
            other => panic!("expected cleanup effect, got {other:?}"),
        };
        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery_token = match runtime_state.next_effect() {
            Some(MakerEffect::Recover { token, .. }) => token,
            other => panic!("expected recovery effect, got {other:?}"),
        };
        let mut resting = vec![resting_quote()];
        let mut inventory_exit_pending = false;
        let mut consecutive_errors = 2;
        let mut next_cycle_is_recovery = false;

        resume_quoting_after_recovery(
            &mut RecoveryIo {
                runtime_state: &mut runtime_state,
                notifier: &notifier,
                client: &client,
                session: None,
                resting: &mut resting,
                inventory_exit_pending: &mut inventory_exit_pending,
                consecutive_errors: &mut consecutive_errors,
                next_cycle_is_recovery: &mut next_cycle_is_recovery,
                symbol: "BTC-USD",
                cycle: 7,
                output_format: OutputFormat::Quiet,
            },
            ResumeSpec {
                recovery_token,
                observed: 0.0,
                projection_reset: ProjectionReset::PreservePendingAcks,
                clear_resting: true,
                reset_consecutive_errors: true,
                recovered_note: None,
                notice: RiskNotice {
                    kind: "position_reconciliation",
                    severity: "resolved",
                    event: "recovered",
                    message: "test resume",
                    symbol: "BTC-USD",
                    cycle: 7,
                    position_before: None,
                    position_after: None,
                    expected: Some(0.0),
                    observed: Some(0.0),
                },
            },
        )
        .await;

        assert!(resting.is_empty());
        assert_eq!(consecutive_errors, 0);
        assert!(next_cycle_is_recovery);
        assert!(
            matches!(runtime_state.next_effect(), Some(MakerEffect::RunCycle(_))),
            "resume must schedule the next quoting cycle"
        );
    }

    /// Invariant: the projection-reset knob keeps its per-flow semantics —
    /// preserving pending request lifecycles for late acks, or dropping them
    /// when the placement channel is being replaced.
    #[test]
    fn projection_reset_knobs_preserve_or_drop_pending_requests() {
        let mut projection = projection_with_pending(&["request-1"]);
        reset_projection(&mut projection, ProjectionReset::PreservePendingAcks);
        assert!(
            projection.has_pending_request_lifecycle("request-1"),
            "PreservePendingAcks must keep pending request lifecycles"
        );

        let mut projection = projection_with_pending(&["request-1"]);
        reset_projection(&mut projection, ProjectionReset::DropPendingRequests);
        assert!(
            !projection.has_pending_request_lifecycle("request-1"),
            "DropPendingRequests must clear pending request lifecycles"
        );
    }
}
