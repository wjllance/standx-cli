//! Pure maker runtime state transitions; the CLI executes queued effects.

use std::collections::VecDeque;

const MAX_UNMATCHED_ORDER_RESPONSES: usize = 256;
// Benign cancel/exit/late placement acknowledgements age out by cycle; only a
// sustained burst of genuinely uncorrelated responses may overflow and freeze.
const UNMATCHED_ORDER_RESPONSE_TTL_CYCLES: u64 = 8;
const MAX_CONSECUTIVE_CYCLE_ERRORS: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Starting,
    Ready,
    Frozen { reason: String },
    Stopping { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    Cycle,
    Cleanup,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkToken {
    pub generation: u64,
    pub kind: WorkKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryTarget {
    AccountStream,
    OrderResponse,
    PositionReconciliation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStopReason {
    CtrlC,
    OrderResponse(String),
    PositionReconciliation(String),
    ConsecutiveCycleErrors(String),
    StopLoss(String),
    CleanupFailure {
        target: RecoveryTarget,
        reason: String,
    },
}

impl RuntimeStopReason {
    pub fn detail(&self) -> String {
        match self {
            Self::CtrlC => "Ctrl+C".to_string(),
            Self::OrderResponse(reason)
            | Self::PositionReconciliation(reason)
            | Self::ConsecutiveCycleErrors(reason)
            | Self::StopLoss(reason) => reason.clone(),
            Self::CleanupFailure { reason, .. } => reason.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MakerEvent {
    StartupReady,
    Timer,
    MarketChanged,
    CycleCompleted(WorkToken),
    CycleFailed { token: WorkToken, reason: String },
    AccountStreamDisconnected(String),
    OrderResponseDisconnected(String),
    PositionMismatch,
    CleanupCompleted(WorkToken),
    CleanupFailed { token: WorkToken, reason: String },
    RecoverySucceeded(WorkToken),
    RecoveryFailed { token: WorkToken, reason: String },
    OrderResponseUnmatched { request_id: String, cycle: u64 },
    OrderResponseMatched(String),
    StopRequested(RuntimeStopReason),
    CtrlC,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MakerEffect {
    RunCycle(WorkToken),
    AbortInFlight(WorkToken),
    CommitCycle(WorkToken),
    Cleanup {
        token: WorkToken,
        target: RecoveryTarget,
    },
    Recover {
        token: WorkToken,
        target: RecoveryTarget,
    },
    Stop(RuntimeStopReason),
}

#[derive(Debug)]
pub struct MakerState {
    phase: RuntimePhase,
    generation: u64,
    in_flight: Option<WorkToken>,
    recovery_target: Option<RecoveryTarget>,
    replan_requested: bool,
    consecutive_cycle_errors: u32,
    unmatched_order_responses: VecDeque<(u64, String)>,
    effects: VecDeque<MakerEffect>,
}

impl MakerState {
    pub fn starting() -> Self {
        Self {
            phase: RuntimePhase::Starting,
            generation: 0,
            in_flight: None,
            recovery_target: None,
            replan_requested: false,
            consecutive_cycle_errors: 0,
            unmatched_order_responses: VecDeque::new(),
            effects: VecDeque::new(),
        }
    }

    pub fn phase(&self) -> &RuntimePhase {
        &self.phase
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_frozen(&self) -> bool {
        matches!(self.phase, RuntimePhase::Frozen { .. })
    }

    /// Applies an event and retains every resulting effect until the CLI
    /// explicitly drains it with [`Self::next_effect`].
    pub fn handle(&mut self, event: MakerEvent) {
        let effects = self.transition(event);
        self.effects.extend(effects);
    }

    pub fn next_effect(&mut self) -> Option<MakerEffect> {
        self.effects.pop_front()
    }

    pub fn pending_effect(&self) -> Option<&MakerEffect> {
        self.effects.front()
    }

    fn transition(&mut self, event: MakerEvent) -> Vec<MakerEffect> {
        match event {
            MakerEvent::StartupReady => {
                if !matches!(self.phase, RuntimePhase::Starting) {
                    return Vec::new();
                }
                self.phase = RuntimePhase::Ready;
                self.request_cycle()
            }
            MakerEvent::Timer | MakerEvent::MarketChanged => {
                if !matches!(self.phase, RuntimePhase::Ready) {
                    return Vec::new();
                }
                if self.in_flight.is_some() {
                    self.replan_requested = true;
                    Vec::new()
                } else {
                    self.request_cycle()
                }
            }
            MakerEvent::CycleCompleted(token) => {
                if !self.matches_in_flight(token, WorkKind::Cycle) {
                    return Vec::new();
                }
                self.in_flight = None;
                self.consecutive_cycle_errors = 0;
                let mut effects = vec![MakerEffect::CommitCycle(token)];
                if self.replan_requested && matches!(self.phase, RuntimePhase::Ready) {
                    self.replan_requested = false;
                    effects.extend(self.request_cycle());
                }
                effects
            }
            MakerEvent::CycleFailed { token, reason } => {
                if !self.matches_in_flight(token, WorkKind::Cycle) {
                    return Vec::new();
                }
                self.in_flight = None;
                self.consecutive_cycle_errors = self.consecutive_cycle_errors.saturating_add(1);
                if self.consecutive_cycle_errors >= MAX_CONSECUTIVE_CYCLE_ERRORS {
                    self.stop(RuntimeStopReason::ConsecutiveCycleErrors(reason))
                } else {
                    Vec::new()
                }
            }
            MakerEvent::AccountStreamDisconnected(reason) => {
                self.freeze(reason, RecoveryTarget::AccountStream)
            }
            MakerEvent::OrderResponseDisconnected(reason) => {
                self.freeze(reason, RecoveryTarget::OrderResponse)
            }
            MakerEvent::PositionMismatch => self.freeze(
                "position mismatch".to_string(),
                RecoveryTarget::PositionReconciliation,
            ),
            MakerEvent::CleanupCompleted(token) => {
                if !self.matches_in_flight(token, WorkKind::Cleanup) {
                    return Vec::new();
                }
                let Some(target) = self.recovery_target else {
                    return self.stop(RuntimeStopReason::CleanupFailure {
                        target: RecoveryTarget::PositionReconciliation,
                        reason: "cleanup completed without a recovery target".to_string(),
                    });
                };
                let token = WorkToken {
                    generation: self.generation,
                    kind: WorkKind::Recovery,
                };
                self.in_flight = Some(token);
                vec![MakerEffect::Recover { token, target }]
            }
            MakerEvent::CleanupFailed { token, reason } => {
                if !self.matches_in_flight(token, WorkKind::Cleanup) {
                    return Vec::new();
                }
                self.in_flight = None;
                self.stop(RuntimeStopReason::CleanupFailure {
                    target: self
                        .recovery_target
                        .unwrap_or(RecoveryTarget::PositionReconciliation),
                    reason,
                })
            }
            MakerEvent::RecoverySucceeded(token) => {
                if !self.matches_in_flight(token, WorkKind::Recovery) {
                    return Vec::new();
                }
                self.in_flight = None;
                self.recovery_target = None;
                self.unmatched_order_responses.clear();
                self.consecutive_cycle_errors = 0;
                self.phase = RuntimePhase::Ready;
                self.request_cycle()
            }
            MakerEvent::RecoveryFailed { token, reason } => {
                if !self.matches_in_flight(token, WorkKind::Recovery) {
                    return Vec::new();
                }
                self.in_flight = None;
                let stop = match self.recovery_target {
                    Some(RecoveryTarget::OrderResponse) => RuntimeStopReason::OrderResponse(reason),
                    _ => RuntimeStopReason::PositionReconciliation(reason),
                };
                self.stop(stop)
            }
            MakerEvent::OrderResponseUnmatched { request_id, cycle } => {
                self.unmatched_order_responses
                    .push_back((cycle, request_id));
                self.unmatched_order_responses.retain(|(seen, _)| {
                    cycle.saturating_sub(*seen) <= UNMATCHED_ORDER_RESPONSE_TTL_CYCLES
                });
                if self.unmatched_order_responses.len() > MAX_UNMATCHED_ORDER_RESPONSES {
                    self.freeze(
                        "unmatched order-response buffer overflow".to_string(),
                        RecoveryTarget::OrderResponse,
                    )
                } else {
                    Vec::new()
                }
            }
            MakerEvent::OrderResponseMatched(request_id) => {
                if let Some(index) = self
                    .unmatched_order_responses
                    .iter()
                    .position(|(_, candidate)| candidate == &request_id)
                {
                    self.unmatched_order_responses.remove(index);
                }
                Vec::new()
            }
            MakerEvent::StopRequested(reason) => self.stop(reason),
            MakerEvent::CtrlC => self.stop(RuntimeStopReason::CtrlC),
        }
    }

    fn matches_in_flight(&self, token: WorkToken, kind: WorkKind) -> bool {
        token.generation == self.generation && token.kind == kind && self.in_flight == Some(token)
    }

    fn request_cycle(&mut self) -> Vec<MakerEffect> {
        let token = WorkToken {
            generation: self.generation,
            kind: WorkKind::Cycle,
        };
        self.in_flight = Some(token);
        vec![MakerEffect::RunCycle(token)]
    }

    fn freeze(&mut self, reason: String, target: RecoveryTarget) -> Vec<MakerEffect> {
        if matches!(
            self.phase,
            RuntimePhase::Frozen { .. } | RuntimePhase::Stopping { .. }
        ) {
            return Vec::new();
        }
        self.effects.clear();
        self.generation = self.generation.saturating_add(1);
        self.replan_requested = false;
        self.unmatched_order_responses.clear();
        let mut effects = self.abort_effect();
        self.phase = RuntimePhase::Frozen { reason };
        self.recovery_target = Some(target);
        let token = WorkToken {
            generation: self.generation,
            kind: WorkKind::Cleanup,
        };
        self.in_flight = Some(token);
        effects.push(MakerEffect::Cleanup { token, target });
        effects
    }

    fn stop(&mut self, reason: RuntimeStopReason) -> Vec<MakerEffect> {
        if matches!(self.phase, RuntimePhase::Stopping { .. }) {
            return Vec::new();
        }
        self.effects.clear();
        self.generation = self.generation.saturating_add(1);
        self.replan_requested = false;
        self.recovery_target = None;
        let mut effects = self.abort_effect();
        self.phase = RuntimePhase::Stopping {
            reason: reason.detail(),
        };
        effects.push(MakerEffect::Stop(reason));
        effects
    }

    fn abort_effect(&mut self) -> Vec<MakerEffect> {
        self.in_flight
            .take()
            .map(MakerEffect::AbortInFlight)
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next_cycle(state: &mut MakerState) -> WorkToken {
        match state.next_effect().expect("cycle effect") {
            MakerEffect::RunCycle(token) => token,
            effect => panic!("expected cycle, got {effect:?}"),
        }
    }

    #[test]
    fn effects_are_retained_until_explicitly_drained() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let token = next_cycle(&mut state);
        assert_eq!(token.kind, WorkKind::Cycle);
        assert!(state.next_effect().is_none());
    }

    #[test]
    fn critical_events_abort_stale_work_and_clean_up() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let token = next_cycle(&mut state);
        state.handle(MakerEvent::PositionMismatch);
        assert_eq!(state.generation(), 1);
        assert!(matches!(state.phase(), RuntimePhase::Frozen { .. }));
        assert_eq!(state.next_effect(), Some(MakerEffect::AbortInFlight(token)));
        let cleanup = match state.next_effect().expect("cleanup") {
            MakerEffect::Cleanup { token, target } => {
                assert_eq!(target, RecoveryTarget::PositionReconciliation);
                token
            }
            effect => panic!("expected cleanup, got {effect:?}"),
        };
        state.handle(MakerEvent::CycleCompleted(token));
        assert!(state.next_effect().is_none());
        state.handle(MakerEvent::CleanupCompleted(cleanup));
        assert!(matches!(
            state.next_effect(),
            Some(MakerEffect::Recover {
                target: RecoveryTarget::PositionReconciliation,
                ..
            })
        ));
    }

    #[test]
    fn coalesced_timer_commits_then_replans() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let token = next_cycle(&mut state);
        state.handle(MakerEvent::Timer);
        state.handle(MakerEvent::MarketChanged);
        assert!(state.next_effect().is_none());
        state.handle(MakerEvent::CycleCompleted(token));
        assert_eq!(state.next_effect(), Some(MakerEffect::CommitCycle(token)));
        assert!(matches!(
            state.next_effect(),
            Some(MakerEffect::RunCycle(_))
        ));
    }

    #[test]
    fn each_recovery_target_runs_only_after_cleanup() {
        for (event, expected_target) in [
            (
                MakerEvent::AccountStreamDisconnected("closed".to_string()),
                RecoveryTarget::AccountStream,
            ),
            (
                MakerEvent::OrderResponseDisconnected("closed".to_string()),
                RecoveryTarget::OrderResponse,
            ),
            (
                MakerEvent::PositionMismatch,
                RecoveryTarget::PositionReconciliation,
            ),
        ] {
            let mut state = MakerState::starting();
            state.handle(MakerEvent::StartupReady);
            let _ = next_cycle(&mut state);
            state.handle(event);
            let _ = state.next_effect();
            let cleanup = match state.next_effect().expect("cleanup") {
                MakerEffect::Cleanup { token, target } => {
                    assert_eq!(target, expected_target);
                    token
                }
                effect => panic!("expected cleanup, got {effect:?}"),
            };
            state.handle(MakerEvent::CleanupCompleted(cleanup));
            let recovery = match state.next_effect().expect("recovery") {
                MakerEffect::Recover { token, target } => {
                    assert_eq!(target, expected_target);
                    token
                }
                effect => panic!("expected recovery, got {effect:?}"),
            };
            state.handle(MakerEvent::RecoverySucceeded(recovery));
            assert!(matches!(
                state.next_effect(),
                Some(MakerEffect::RunCycle(_))
            ));
            assert!(matches!(state.phase(), RuntimePhase::Ready));
        }
    }

    #[test]
    fn cleanup_or_recovery_failure_stops() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let _ = next_cycle(&mut state);
        state.handle(MakerEvent::PositionMismatch);
        let _ = state.next_effect();
        let cleanup = match state.next_effect().unwrap() {
            MakerEffect::Cleanup { token, .. } => token,
            _ => unreachable!(),
        };
        state.handle(MakerEvent::CleanupFailed {
            token: cleanup,
            reason: "residual orders".to_string(),
        });
        assert!(matches!(state.next_effect(), Some(MakerEffect::Stop(_))));

        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let _ = next_cycle(&mut state);
        state.handle(MakerEvent::AccountStreamDisconnected("closed".to_string()));
        let _ = state.next_effect();
        let cleanup = match state.next_effect().unwrap() {
            MakerEffect::Cleanup { token, .. } => token,
            _ => unreachable!(),
        };
        state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery = match state.next_effect().unwrap() {
            MakerEffect::Recover { token, .. } => token,
            _ => unreachable!(),
        };
        state.handle(MakerEvent::RecoveryFailed {
            token: recovery,
            reason: "timeout".to_string(),
        });
        assert!(matches!(state.next_effect(), Some(MakerEffect::Stop(_))));
    }

    #[test]
    fn third_cycle_failure_stops_and_stale_failures_are_ignored() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        for attempt in 0..3 {
            let token = next_cycle(&mut state);
            state.handle(MakerEvent::CycleFailed {
                token,
                reason: format!("failure {attempt}"),
            });
            state.handle(MakerEvent::CycleFailed {
                token,
                reason: "stale duplicate".to_string(),
            });
            if attempt < 2 {
                assert!(state.pending_effect().is_none());
                state.handle(MakerEvent::Timer);
            }
        }
        assert!(matches!(state.next_effect(), Some(MakerEffect::Stop(_))));
    }

    #[test]
    fn ctrl_c_aborts_and_stops() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let token = next_cycle(&mut state);
        state.handle(MakerEvent::CtrlC);
        assert_eq!(state.next_effect(), Some(MakerEffect::AbortInFlight(token)));
        assert_eq!(
            state.next_effect(),
            Some(MakerEffect::Stop(RuntimeStopReason::CtrlC))
        );
    }

    #[test]
    fn unmatched_response_overflow_fails_closed() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let _ = next_cycle(&mut state);
        for index in 0..=MAX_UNMATCHED_ORDER_RESPONSES {
            state.handle(MakerEvent::OrderResponseUnmatched {
                request_id: index.to_string(),
                cycle: 0,
            });
        }
        assert!(state.is_frozen());
        assert!(matches!(
            state.next_effect(),
            Some(MakerEffect::AbortInFlight(_))
        ));
        assert!(matches!(
            state.next_effect(),
            Some(MakerEffect::Cleanup {
                target: RecoveryTarget::OrderResponse,
                ..
            })
        ));
    }

    #[test]
    fn benign_unmatched_responses_decay_over_a_long_session() {
        let mut state = MakerState::starting();
        state.handle(MakerEvent::StartupReady);
        let _ = next_cycle(&mut state);
        for cycle in 0..10_000u64 {
            for seq in 0..4 {
                state.handle(MakerEvent::OrderResponseUnmatched {
                    request_id: format!("cycle-{cycle}-{seq}"),
                    cycle,
                });
            }
            assert!(!state.is_frozen(), "froze at cycle {cycle}");
        }
        assert!(state.unmatched_order_responses.len() <= MAX_UNMATCHED_ORDER_RESPONSES);
    }
}
