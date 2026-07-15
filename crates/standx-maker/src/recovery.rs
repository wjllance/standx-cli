//! Deterministic recovery admission policy for live transport incidents.

use std::collections::VecDeque;

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

    /// Admit one transport-recovery incident at monotonic `now_secs`.
    /// Individual reconnect attempts are deliberately not recorded here; the
    /// caller owns that bounded retry loop for the admitted incident.
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
}
