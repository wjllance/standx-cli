//! Deterministic recovery admission policy for live recovery incidents
//! (transport reconnects and position-mismatch freeze/recover alike).

use std::collections::VecDeque;

/// Why the live runtime entered a cleanup/recovery flow.
///
/// A cycle invalidated by an account event still requires compensating cleanup
/// and authoritative reconciliation, but it is expected during normal fills.
/// Counting it as an incident would eventually stop every healthy active maker.
/// Market conditions that make quoting unsafe are likewise expected to clear
/// without consuming the transport/reconciliation recovery budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryTrigger {
    TransportFailure,
    PositionMismatch,
    CycleInvalidation,
    MarketCondition,
}

impl RecoveryTrigger {
    pub fn meters_circuit(self) -> bool {
        !matches!(self, Self::CycleInvalidation | Self::MarketCondition)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAdmission {
    Admitted {
        incidents: u32,
        limit: u32,
        window_secs: u64,
    },
    CircuitOpen {
        incidents: u32,
        limit: u32,
        window_secs: u64,
    },
}

impl RecoveryAdmission {
    pub fn is_admitted(self) -> bool {
        matches!(self, Self::Admitted { .. })
    }
}

#[derive(Debug)]
pub struct RecoveryCircuitBreaker {
    limit: u32,
    window_secs: u64,
    incidents: VecDeque<u64>,
}

impl RecoveryCircuitBreaker {
    pub fn new(limit: u32, window_secs: u64) -> Self {
        Self {
            limit,
            window_secs,
            incidents: VecDeque::new(),
        }
    }

    /// Admit one recovery incident at monotonic `now_secs`.
    /// The individual recovery attempts inside an admitted incident (reconnect
    /// retries, the position-mismatch backfill loop) are deliberately not
    /// recorded here; the caller owns that bounded work for the admitted incident.
    pub fn admit(&mut self, now_secs: u64) -> RecoveryAdmission {
        while self
            .incidents
            .front()
            .is_some_and(|started| now_secs.saturating_sub(*started) >= self.window_secs)
        {
            self.incidents.pop_front();
        }
        let incidents = self.incidents.len() as u32;
        if incidents >= self.limit {
            return RecoveryAdmission::CircuitOpen {
                incidents,
                limit: self.limit,
                window_secs: self.window_secs,
            };
        }
        self.incidents.push_back(now_secs);
        RecoveryAdmission::Admitted {
            incidents: incidents + 1,
            limit: self.limit,
            window_secs: self.window_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incidents_are_bounded_inside_one_window() {
        let mut breaker = RecoveryCircuitBreaker::new(2, 60);
        assert!(breaker.admit(10).is_admitted());
        assert_eq!(breaker.incidents.len(), 1);
        assert!(breaker.admit(20).is_admitted());
        assert_eq!(breaker.incidents.len(), 2);
        assert!(matches!(
            breaker.admit(30),
            RecoveryAdmission::CircuitOpen { incidents: 2, .. }
        ));
    }

    #[test]
    fn incidents_expire_at_the_rolling_window_boundary() {
        let mut breaker = RecoveryCircuitBreaker::new(1, 60);
        assert!(breaker.admit(10).is_admitted());
        assert!(!breaker.admit(69).is_admitted());
        assert!(breaker.admit(70).is_admitted());
    }

    #[test]
    fn zero_limit_is_fail_closed() {
        let mut breaker = RecoveryCircuitBreaker::new(0, 60);
        assert!(matches!(
            breaker.admit(0),
            RecoveryAdmission::CircuitOpen {
                incidents: 0,
                limit: 0,
                ..
            }
        ));
    }

    #[test]
    fn expected_runtime_conditions_do_not_meter_the_incident_circuit() {
        assert!(!RecoveryTrigger::CycleInvalidation.meters_circuit());
        assert!(!RecoveryTrigger::MarketCondition.meters_circuit());
        assert!(RecoveryTrigger::PositionMismatch.meters_circuit());
        assert!(RecoveryTrigger::TransportFailure.meters_circuit());
    }
}
