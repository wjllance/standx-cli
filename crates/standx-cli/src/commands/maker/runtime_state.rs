use std::collections::VecDeque;

const MAX_UNMATCHED_ORDER_RESPONSES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RuntimePhase {
    Starting,
    Ready,
    Frozen { reason: String },
    Stopping { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkKind {
    Snapshot,
    Cleanup,
    Reconnect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WorkToken {
    pub(super) generation: u64,
    pub(super) kind: WorkKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MakerEvent {
    StartupReady,
    Timer,
    MarketChanged,
    WorkFinished(WorkToken),
    AccountStreamDisconnected(String),
    OrderResponseDisconnected(String),
    PositionMismatch,
    CleanupComplete,
    RecoverySucceeded,
    RecoveryFailed(String),
    OrderResponseUnmatched(String),
    OrderResponseMatched(String),
    CtrlC,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MakerEffect {
    FetchSnapshot(WorkToken),
    AbortInFlight(WorkToken),
    Cleanup(WorkToken),
    Reconnect(WorkToken),
    Stop,
}

#[derive(Debug)]
pub(super) struct MakerState {
    phase: RuntimePhase,
    generation: u64,
    in_flight: Option<WorkToken>,
    replan_requested: bool,
    unmatched_order_responses: VecDeque<String>,
}

impl MakerState {
    pub(super) fn starting() -> Self {
        Self {
            phase: RuntimePhase::Starting,
            generation: 0,
            in_flight: None,
            replan_requested: false,
            unmatched_order_responses: VecDeque::new(),
        }
    }

    pub(super) fn in_flight(&self) -> Option<WorkToken> {
        self.in_flight
    }

    pub(super) fn is_frozen(&self) -> bool {
        matches!(self.phase, RuntimePhase::Frozen { .. })
    }

    pub(super) fn reduce(&mut self, event: MakerEvent) -> Vec<MakerEffect> {
        match event {
            MakerEvent::StartupReady => {
                self.phase = RuntimePhase::Ready;
                self.request_snapshot()
            }
            MakerEvent::Timer | MakerEvent::MarketChanged => {
                if !matches!(self.phase, RuntimePhase::Ready) {
                    return Vec::new();
                }
                if self.in_flight.is_some() {
                    self.replan_requested = true;
                    Vec::new()
                } else {
                    self.request_snapshot()
                }
            }
            MakerEvent::WorkFinished(token) => {
                if token.generation != self.generation || self.in_flight != Some(token) {
                    return Vec::new();
                }
                self.in_flight = None;
                if self.replan_requested && matches!(self.phase, RuntimePhase::Ready) {
                    self.replan_requested = false;
                    self.request_snapshot()
                } else {
                    Vec::new()
                }
            }
            MakerEvent::AccountStreamDisconnected(reason)
            | MakerEvent::OrderResponseDisconnected(reason)
            | MakerEvent::RecoveryFailed(reason) => self.freeze(reason),
            MakerEvent::PositionMismatch => self.freeze("position mismatch".to_string()),
            MakerEvent::CleanupComplete => {
                if !matches!(self.phase, RuntimePhase::Frozen { .. }) {
                    return Vec::new();
                }
                let token = WorkToken {
                    generation: self.generation,
                    kind: WorkKind::Reconnect,
                };
                self.in_flight = Some(token);
                vec![MakerEffect::Reconnect(token)]
            }
            MakerEvent::RecoverySucceeded => {
                self.in_flight = None;
                self.unmatched_order_responses.clear();
                self.phase = RuntimePhase::Ready;
                self.request_snapshot()
            }
            MakerEvent::OrderResponseUnmatched(request_id) => {
                self.unmatched_order_responses.push_back(request_id);
                if self.unmatched_order_responses.len() > MAX_UNMATCHED_ORDER_RESPONSES {
                    self.freeze("unmatched order-response buffer overflow".to_string())
                } else {
                    Vec::new()
                }
            }
            MakerEvent::OrderResponseMatched(request_id) => {
                if let Some(index) = self
                    .unmatched_order_responses
                    .iter()
                    .position(|candidate| candidate == &request_id)
                {
                    self.unmatched_order_responses.remove(index);
                }
                Vec::new()
            }
            MakerEvent::CtrlC => {
                self.generation = self.generation.saturating_add(1);
                let mut effects = self.abort_effect();
                self.phase = RuntimePhase::Stopping {
                    reason: "Ctrl+C".to_string(),
                };
                effects.push(MakerEffect::Stop);
                effects
            }
        }
    }

    fn request_snapshot(&mut self) -> Vec<MakerEffect> {
        let token = WorkToken {
            generation: self.generation,
            kind: WorkKind::Snapshot,
        };
        self.in_flight = Some(token);
        vec![MakerEffect::FetchSnapshot(token)]
    }

    fn freeze(&mut self, reason: String) -> Vec<MakerEffect> {
        if matches!(self.phase, RuntimePhase::Stopping { .. }) {
            return Vec::new();
        }
        self.generation = self.generation.saturating_add(1);
        self.replan_requested = false;
        self.unmatched_order_responses.clear();
        let mut effects = self.abort_effect();
        self.phase = RuntimePhase::Frozen { reason };
        let cleanup = WorkToken {
            generation: self.generation,
            kind: WorkKind::Cleanup,
        };
        self.in_flight = Some(cleanup);
        effects.push(MakerEffect::Cleanup(cleanup));
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

    #[test]
    fn coalesces_market_and_timer_events_while_work_is_in_flight() {
        let mut state = MakerState::starting();
        let effects = state.reduce(MakerEvent::StartupReady);
        let token = match effects.as_slice() {
            [MakerEffect::FetchSnapshot(token)] => *token,
            other => panic!("unexpected effects: {other:?}"),
        };
        assert!(state.reduce(MakerEvent::Timer).is_empty());
        assert!(state.reduce(MakerEvent::MarketChanged).is_empty());
        assert_eq!(
            state.reduce(MakerEvent::WorkFinished(token)),
            vec![MakerEffect::FetchSnapshot(token)]
        );
    }

    #[test]
    fn critical_event_invalidates_generation_and_cleans_up() {
        let mut state = MakerState::starting();
        let effects = state.reduce(MakerEvent::StartupReady);
        let old = match effects[0] {
            MakerEffect::FetchSnapshot(token) => token,
            _ => unreachable!(),
        };
        let effects = state.reduce(MakerEvent::PositionMismatch);
        assert_eq!(state.generation, 1);
        assert!(matches!(state.phase, RuntimePhase::Frozen { .. }));
        assert_eq!(effects[0], MakerEffect::AbortInFlight(old));
        assert!(matches!(effects[1], MakerEffect::Cleanup(_)));
        assert!(state.reduce(MakerEvent::WorkFinished(old)).is_empty());
    }

    #[test]
    fn recovery_requires_cleanup_then_reconnect() {
        let mut state = MakerState::starting();
        state.reduce(MakerEvent::StartupReady);
        state.reduce(MakerEvent::AccountStreamDisconnected("closed".to_string()));
        let effects = state.reduce(MakerEvent::CleanupComplete);
        assert!(matches!(effects.as_slice(), [MakerEffect::Reconnect(_)]));
        let effects = state.reduce(MakerEvent::RecoverySucceeded);
        assert!(matches!(state.phase, RuntimePhase::Ready));
        assert!(matches!(
            effects.as_slice(),
            [MakerEffect::FetchSnapshot(_)]
        ));
    }

    #[test]
    fn unmatched_response_buffer_fails_closed() {
        let mut state = MakerState::starting();
        state.reduce(MakerEvent::StartupReady);
        for index in 0..MAX_UNMATCHED_ORDER_RESPONSES {
            assert!(state
                .reduce(MakerEvent::OrderResponseUnmatched(index.to_string()))
                .is_empty());
        }
        let effects = state.reduce(MakerEvent::OrderResponseUnmatched("overflow".to_string()));
        assert!(matches!(state.phase, RuntimePhase::Frozen { .. }));
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, MakerEffect::Cleanup(_))));
    }
}
