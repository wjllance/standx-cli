//! Pure market-data degradation and recovery policy.

/// Consecutive unhealthy observations required before quoting may be frozen.
pub const MARKET_DATA_BAD_OBSERVATIONS_TO_DEGRADE: u32 = 3;
/// Unhealthy observations must also persist for this wall-clock grace period.
pub const MARKET_DATA_BAD_GRACE_MS: u64 = 15_000;
/// Distinct coherent snapshots required before recovery may be confirmed.
pub const MARKET_DATA_COHERENT_SNAPSHOTS_TO_RECOVER: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketDataObservation {
    Coherent,
    RestFallback,
    MarkMidDivergence,
    InvalidSnapshot,
    FeedIdle,
}

impl MarketDataObservation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coherent => "coherent",
            Self::RestFallback => "rest_fallback",
            Self::MarkMidDivergence => "mark_mid_divergence",
            Self::InvalidSnapshot => "invalid_snapshot",
            Self::FeedIdle => "feed_idle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketDataTransition {
    Healthy,
    Grace {
        issue: MarketDataObservation,
        consecutive: u32,
        bad_for_ms: u64,
    },
    EnteredDegraded {
        issue: MarketDataObservation,
        consecutive: u32,
        bad_for_ms: u64,
    },
    Degraded {
        issue: MarketDataObservation,
    },
    Recovering {
        coherent: u32,
        required: u32,
    },
    RecoveryReady,
    Recovered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketDataPhase {
    Healthy,
    Grace {
        first_bad_ms: u64,
        consecutive: u32,
        issue: MarketDataObservation,
    },
    Degraded {
        issue: MarketDataObservation,
        coherent: u32,
    },
}

/// Stateful policy driven only by normalized observations and a monotonic
/// elapsed-millisecond clock supplied by the caller.
#[derive(Debug)]
pub struct MarketDataHealth {
    phase: MarketDataPhase,
    bad_observations_to_degrade: u32,
    bad_grace_ms: u64,
    coherent_snapshots_to_recover: u32,
}

impl Default for MarketDataHealth {
    fn default() -> Self {
        Self::new(
            MARKET_DATA_BAD_OBSERVATIONS_TO_DEGRADE,
            MARKET_DATA_BAD_GRACE_MS,
            MARKET_DATA_COHERENT_SNAPSHOTS_TO_RECOVER,
        )
    }
}

impl MarketDataHealth {
    pub fn new(
        bad_observations_to_degrade: u32,
        bad_grace_ms: u64,
        coherent_snapshots_to_recover: u32,
    ) -> Self {
        Self {
            phase: MarketDataPhase::Healthy,
            bad_observations_to_degrade: bad_observations_to_degrade.max(1),
            bad_grace_ms,
            coherent_snapshots_to_recover: coherent_snapshots_to_recover.max(1),
        }
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self.phase, MarketDataPhase::Degraded { .. })
    }

    pub fn observe(
        &mut self,
        now_ms: u64,
        observation: MarketDataObservation,
    ) -> MarketDataTransition {
        match (self.phase, observation) {
            (MarketDataPhase::Healthy, MarketDataObservation::Coherent) => {
                MarketDataTransition::Healthy
            }
            (MarketDataPhase::Healthy, issue) => self.begin_bad(now_ms, issue),
            (MarketDataPhase::Grace { .. }, MarketDataObservation::Coherent) => {
                self.phase = MarketDataPhase::Healthy;
                MarketDataTransition::Healthy
            }
            (
                MarketDataPhase::Grace {
                    first_bad_ms,
                    consecutive,
                    ..
                },
                issue,
            ) => {
                let consecutive = consecutive.saturating_add(1);
                let bad_for_ms = now_ms.saturating_sub(first_bad_ms);
                if issue == MarketDataObservation::FeedIdle
                    || (consecutive >= self.bad_observations_to_degrade
                        && bad_for_ms >= self.bad_grace_ms)
                {
                    self.phase = MarketDataPhase::Degraded { issue, coherent: 0 };
                    MarketDataTransition::EnteredDegraded {
                        issue,
                        consecutive,
                        bad_for_ms,
                    }
                } else {
                    self.phase = MarketDataPhase::Grace {
                        first_bad_ms,
                        consecutive,
                        issue,
                    };
                    MarketDataTransition::Grace {
                        issue,
                        consecutive,
                        bad_for_ms,
                    }
                }
            }
            (MarketDataPhase::Degraded { issue, coherent }, MarketDataObservation::Coherent) => {
                let coherent = coherent.saturating_add(1);
                self.phase = MarketDataPhase::Degraded { issue, coherent };
                if coherent >= self.coherent_snapshots_to_recover {
                    MarketDataTransition::RecoveryReady
                } else {
                    MarketDataTransition::Recovering {
                        coherent,
                        required: self.coherent_snapshots_to_recover,
                    }
                }
            }
            (MarketDataPhase::Degraded { .. }, issue) => {
                self.phase = MarketDataPhase::Degraded { issue, coherent: 0 };
                MarketDataTransition::Degraded { issue }
            }
        }
    }

    /// Confirms recovery only after the effect executor has independently
    /// verified the venue maker book is empty and the latest snapshot remains
    /// safe. Until then the phase stays degraded and placements remain frozen.
    pub fn confirm_recovered(&mut self) -> MarketDataTransition {
        match self.phase {
            MarketDataPhase::Degraded { coherent, .. }
                if coherent >= self.coherent_snapshots_to_recover =>
            {
                self.phase = MarketDataPhase::Healthy;
                MarketDataTransition::Recovered
            }
            MarketDataPhase::Degraded { issue, .. } => MarketDataTransition::Degraded { issue },
            MarketDataPhase::Healthy | MarketDataPhase::Grace { .. } => {
                MarketDataTransition::Healthy
            }
        }
    }

    fn begin_bad(&mut self, now_ms: u64, issue: MarketDataObservation) -> MarketDataTransition {
        if issue == MarketDataObservation::FeedIdle
            || (self.bad_observations_to_degrade == 1 && self.bad_grace_ms == 0)
        {
            self.phase = MarketDataPhase::Degraded { issue, coherent: 0 };
            return MarketDataTransition::EnteredDegraded {
                issue,
                consecutive: 1,
                bad_for_ms: 0,
            };
        }
        self.phase = MarketDataPhase::Grace {
            first_bad_ms: now_ms,
            consecutive: 1,
            issue,
        };
        MarketDataTransition::Grace {
            issue,
            consecutive: 1,
            bad_for_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_bad_snapshot_clears_without_degrading() {
        let mut health = MarketDataHealth::default();
        assert!(matches!(
            health.observe(100, MarketDataObservation::RestFallback),
            MarketDataTransition::Grace { consecutive: 1, .. }
        ));
        assert_eq!(
            health.observe(200, MarketDataObservation::Coherent),
            MarketDataTransition::Healthy
        );
        assert!(!health.is_degraded());
    }

    #[test]
    fn bad_observations_require_count_and_elapsed_grace_to_degrade() {
        let mut health = MarketDataHealth::default();
        let _ = health.observe(0, MarketDataObservation::RestFallback);
        let _ = health.observe(1_000, MarketDataObservation::MarkMidDivergence);
        assert!(matches!(
            health.observe(2_000, MarketDataObservation::RestFallback),
            MarketDataTransition::Grace {
                consecutive: 3,
                bad_for_ms: 2_000,
                ..
            }
        ));
        assert!(!health.is_degraded());
        assert!(matches!(
            health.observe(15_000, MarketDataObservation::RestFallback),
            MarketDataTransition::EnteredDegraded {
                consecutive: 4,
                bad_for_ms: 15_000,
                ..
            }
        ));

        let mut sparse = MarketDataHealth::default();
        let _ = sparse.observe(0, MarketDataObservation::RestFallback);
        assert!(matches!(
            sparse.observe(15_000, MarketDataObservation::RestFallback),
            MarketDataTransition::Grace {
                consecutive: 2,
                bad_for_ms: 15_000,
                ..
            }
        ));
        assert!(!sparse.is_degraded());
    }

    #[test]
    fn rapid_divergence_updates_do_not_bypass_grace() {
        let mut health = MarketDataHealth::default();
        let _ = health.observe(0, MarketDataObservation::MarkMidDivergence);
        let _ = health.observe(100, MarketDataObservation::MarkMidDivergence);
        assert!(matches!(
            health.observe(251, MarketDataObservation::MarkMidDivergence),
            MarketDataTransition::Grace {
                consecutive: 3,
                bad_for_ms: 251,
                ..
            }
        ));
        assert!(!health.is_degraded());
    }

    #[test]
    fn feed_idle_degrades_immediately() {
        let mut health = MarketDataHealth::default();
        assert!(matches!(
            health.observe(1, MarketDataObservation::FeedIdle),
            MarketDataTransition::EnteredDegraded {
                issue: MarketDataObservation::FeedIdle,
                ..
            }
        ));
    }

    #[test]
    fn recovery_requires_three_coherent_observations_and_explicit_confirmation() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::RestFallback);
        assert!(health.is_degraded());
        assert!(matches!(
            health.observe(1, MarketDataObservation::Coherent),
            MarketDataTransition::Recovering { coherent: 1, .. }
        ));
        let _ = health.observe(2, MarketDataObservation::Coherent);
        assert_eq!(
            health.observe(3, MarketDataObservation::Coherent),
            MarketDataTransition::RecoveryReady
        );
        assert!(health.is_degraded());
        assert_eq!(health.confirm_recovered(), MarketDataTransition::Recovered);
        assert!(!health.is_degraded());
    }

    #[test]
    fn bad_observation_resets_recovery_streak() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::RestFallback);
        let _ = health.observe(1, MarketDataObservation::Coherent);
        let _ = health.observe(2, MarketDataObservation::Coherent);
        assert!(matches!(
            health.observe(3, MarketDataObservation::InvalidSnapshot),
            MarketDataTransition::Degraded { .. }
        ));
        assert!(matches!(
            health.observe(4, MarketDataObservation::Coherent),
            MarketDataTransition::Recovering { coherent: 1, .. }
        ));
    }
}
