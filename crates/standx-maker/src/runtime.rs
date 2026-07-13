//! Pure maker runtime state transitions; the CLI executes returned effects.

use std::collections::VecDeque;

const MAX_UNMATCHED_ORDER_RESPONSES: usize = 256;
// Benign responses (cancel acks, the reduce-only inventory-exit ack, late acks
// for pending places that already expired) carry a request_id with no matching
// pending place, but arrive at a bounded rate. Age unmatched entries out by
// cycle so those steady sources cannot grow the buffer without limit; only a
// sustained flood of genuinely-uncorrelated responses within the window can
// still overflow and fail closed.
const UNMATCHED_ORDER_RESPONSE_TTL_CYCLES: u64 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Starting,
    Ready,
    Frozen { reason: String },
    Stopping { reason: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    Snapshot,
    Cleanup,
    Reconnect,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkToken {
    pub generation: u64,
    pub kind: WorkKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MakerEvent {
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
    OrderResponseUnmatched { request_id: String, cycle: u64 },
    OrderResponseMatched(String),
    CtrlC,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MakerEffect {
    FetchSnapshot(WorkToken),
    AbortInFlight(WorkToken),
    Cleanup(WorkToken),
    Reconnect(WorkToken),
    Stop,
}
#[derive(Debug)]
pub struct MakerState {
    phase: RuntimePhase,
    generation: u64,
    in_flight: Option<WorkToken>,
    replan_requested: bool,
    unmatched_order_responses: VecDeque<(u64, String)>,
}
impl MakerState {
    pub fn starting() -> Self {
        Self {
            phase: RuntimePhase::Starting,
            generation: 0,
            in_flight: None,
            replan_requested: false,
            unmatched_order_responses: VecDeque::new(),
        }
    }
    pub fn phase(&self) -> &RuntimePhase {
        &self.phase
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn in_flight(&self) -> Option<WorkToken> {
        self.in_flight
    }
    pub fn is_frozen(&self) -> bool {
        matches!(self.phase, RuntimePhase::Frozen { .. })
    }
    pub fn reduce(&mut self, event: MakerEvent) -> Vec<MakerEffect> {
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
            MakerEvent::OrderResponseUnmatched { request_id, cycle } => {
                self.unmatched_order_responses
                    .push_back((cycle, request_id));
                self.unmatched_order_responses.retain(|(seen, _)| {
                    cycle.saturating_sub(*seen) <= UNMATCHED_ORDER_RESPONSE_TTL_CYCLES
                });
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
                    .position(|(_, candidate)| candidate == &request_id)
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
    fn critical_events_abort_stale_work_and_clean_up() {
        let mut state = MakerState::starting();
        let token = match state.reduce(MakerEvent::StartupReady)[0] {
            MakerEffect::FetchSnapshot(token) => token,
            _ => unreachable!(),
        };
        let effects = state.reduce(MakerEvent::PositionMismatch);
        assert_eq!(state.generation(), 1);
        assert!(matches!(state.phase(), RuntimePhase::Frozen { .. }));
        assert_eq!(effects[0], MakerEffect::AbortInFlight(token));
        assert!(matches!(effects[1], MakerEffect::Cleanup(_)));
        assert!(state.reduce(MakerEvent::WorkFinished(token)).is_empty());
    }
    #[test]
    fn coalesced_timer_replans_after_current_work() {
        let mut state = MakerState::starting();
        let token = match state.reduce(MakerEvent::StartupReady)[0] {
            MakerEffect::FetchSnapshot(token) => token,
            _ => unreachable!(),
        };
        assert!(state.reduce(MakerEvent::Timer).is_empty());
        assert!(state.reduce(MakerEvent::MarketChanged).is_empty());
        assert!(matches!(
            state.reduce(MakerEvent::WorkFinished(token)).as_slice(),
            [MakerEffect::FetchSnapshot(_)]
        ));
    }

    #[test]
    fn recovery_reconnects_only_after_cleanup() {
        let mut state = MakerState::starting();
        state.reduce(MakerEvent::StartupReady);
        state.reduce(MakerEvent::AccountStreamDisconnected("closed".to_string()));
        assert!(matches!(
            state.reduce(MakerEvent::CleanupComplete).as_slice(),
            [MakerEffect::Reconnect(_)]
        ));
        assert!(matches!(
            state.reduce(MakerEvent::RecoverySucceeded).as_slice(),
            [MakerEffect::FetchSnapshot(_)]
        ));
        assert!(matches!(state.phase(), RuntimePhase::Ready));
    }

    #[test]
    fn unmatched_response_overflow_fails_closed() {
        let mut state = MakerState::starting();
        state.reduce(MakerEvent::StartupReady);
        // A sustained flood inside one decay window is a genuine correlation
        // failure and must still fail closed. Sub-threshold pushes emit no
        // effect; the push that crosses the cap freezes and emits Cleanup.
        for index in 0..MAX_UNMATCHED_ORDER_RESPONSES {
            assert!(state
                .reduce(MakerEvent::OrderResponseUnmatched {
                    request_id: index.to_string(),
                    cycle: 0,
                })
                .is_empty());
        }
        let effects = state.reduce(MakerEvent::OrderResponseUnmatched {
            request_id: "overflow".to_string(),
            cycle: 0,
        });
        assert!(state.is_frozen());
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, MakerEffect::Cleanup(_))));
    }

    #[test]
    fn benign_unmatched_responses_decay_over_a_long_session() {
        let mut state = MakerState::starting();
        state.reduce(MakerEvent::StartupReady);
        // Simulate a long live run where every cycle produces a few unmatched
        // responses from benign sources (cancel acks, the inventory-exit ack,
        // late acks for expired pending places). Without decay the buffer would
        // pass 256 after ~a hundred cycles and falsely fail closed; with decay
        // it stays bounded and the session never freezes.
        for cycle in 0..10_000u64 {
            for seq in 0..4 {
                state.reduce(MakerEvent::OrderResponseUnmatched {
                    request_id: format!("cycle-{cycle}-{seq}"),
                    cycle,
                });
            }
            assert!(!state.is_frozen(), "froze at cycle {cycle}");
        }
        assert!(
            state.unmatched_order_responses.len() <= MAX_UNMATCHED_ORDER_RESPONSES,
            "buffer grew unbounded: {}",
            state.unmatched_order_responses.len()
        );
    }
}
