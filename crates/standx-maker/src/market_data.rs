//! Pure market-data degradation and recovery policy.

/// Consecutive unhealthy observations required before quoting may be frozen.
pub const MARKET_DATA_BAD_OBSERVATIONS_TO_DEGRADE: u32 = 3;
/// Unhealthy observations must also persist for this wall-clock grace period.
pub const MARKET_DATA_BAD_GRACE_MS: u64 = 15_000;
/// Distinct paired snapshots required to prove transport or quote recovery.
pub const MARKET_DATA_COHERENT_SNAPSHOTS_TO_RECOVER: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketDataFaultClass {
    MarketState,
    Transport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketDataMode {
    Active,
    Paused,
}

impl MarketDataFaultClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::MarketState => "market_state",
            Self::Transport => "transport",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketDataObservation {
    Coherent,
    RestFallback,
    MarkMidDivergence,
    CrossedBook,
    InvalidSnapshot,
    FeedIdle,
}

impl MarketDataObservation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coherent => "coherent",
            Self::RestFallback => "rest_fallback",
            Self::MarkMidDivergence => "mark_mid_divergence",
            Self::CrossedBook => "crossed_book",
            Self::InvalidSnapshot => "invalid_snapshot",
            Self::FeedIdle => "feed_idle",
        }
    }

    pub const fn fault_class(self) -> Option<MarketDataFaultClass> {
        match self {
            Self::Coherent => None,
            Self::MarkMidDivergence | Self::CrossedBook => Some(MarketDataFaultClass::MarketState),
            Self::RestFallback | Self::InvalidSnapshot | Self::FeedIdle => {
                Some(MarketDataFaultClass::Transport)
            }
        }
    }

    pub const fn transport_healthy(self) -> bool {
        matches!(
            self,
            Self::Coherent | Self::MarkMidDivergence | Self::CrossedBook
        )
    }

    pub const fn quoteable(self) -> bool {
        matches!(self, Self::Coherent)
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
        class: MarketDataFaultClass,
        consecutive: u32,
        bad_for_ms: u64,
    },
    Degraded {
        issue: MarketDataObservation,
        class: MarketDataFaultClass,
    },
    ClassChanged {
        from: MarketDataFaultClass,
        to: MarketDataFaultClass,
        issue: MarketDataObservation,
    },
    Recovering {
        class: MarketDataFaultClass,
        transport_healthy: u32,
        quoteable: u32,
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
        class: MarketDataFaultClass,
        transport_healthy: u32,
        quoteable: u32,
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

    pub fn degraded_class(&self) -> Option<MarketDataFaultClass> {
        match self.phase {
            MarketDataPhase::Degraded { class, .. } => Some(class),
            MarketDataPhase::Healthy | MarketDataPhase::Grace { .. } => None,
        }
    }

    pub fn recovery_ready(&self) -> bool {
        matches!(
            self.phase,
            MarketDataPhase::Degraded { quoteable, .. }
                if quoteable >= self.coherent_snapshots_to_recover
        )
    }

    pub fn quoteable_streak(&self) -> u32 {
        match self.phase {
            MarketDataPhase::Degraded { quoteable, .. } => quoteable,
            MarketDataPhase::Healthy | MarketDataPhase::Grace { .. } => 0,
        }
    }

    pub fn observe(
        &mut self,
        now_ms: u64,
        observation: MarketDataObservation,
    ) -> MarketDataTransition {
        match self.phase {
            MarketDataPhase::Healthy if observation.quoteable() => MarketDataTransition::Healthy,
            MarketDataPhase::Healthy => self.begin_bad(now_ms, observation),
            MarketDataPhase::Grace { .. } if observation.quoteable() => {
                self.phase = MarketDataPhase::Healthy;
                MarketDataTransition::Healthy
            }
            MarketDataPhase::Grace {
                first_bad_ms,
                consecutive,
                ..
            } => {
                let consecutive = consecutive.saturating_add(1);
                let bad_for_ms = now_ms.saturating_sub(first_bad_ms);
                if observation == MarketDataObservation::FeedIdle
                    || (consecutive >= self.bad_observations_to_degrade
                        && bad_for_ms >= self.bad_grace_ms)
                {
                    let class = observation
                        .fault_class()
                        .expect("non-coherent observations have a fault class");
                    self.phase = MarketDataPhase::Degraded {
                        issue: observation,
                        class,
                        transport_healthy: 0,
                        quoteable: 0,
                    };
                    MarketDataTransition::EnteredDegraded {
                        issue: observation,
                        class,
                        consecutive,
                        bad_for_ms,
                    }
                } else {
                    self.phase = MarketDataPhase::Grace {
                        first_bad_ms,
                        consecutive,
                        issue: observation,
                    };
                    MarketDataTransition::Grace {
                        issue: observation,
                        consecutive,
                        bad_for_ms,
                    }
                }
            }
            MarketDataPhase::Degraded {
                issue,
                class,
                transport_healthy,
                quoteable,
            } => self.observe_degraded(observation, issue, class, transport_healthy, quoteable),
        }
    }

    fn observe_degraded(
        &mut self,
        observation: MarketDataObservation,
        prior_issue: MarketDataObservation,
        prior_class: MarketDataFaultClass,
        prior_transport_healthy: u32,
        prior_quoteable: u32,
    ) -> MarketDataTransition {
        if !observation.transport_healthy() {
            let class = MarketDataFaultClass::Transport;
            self.phase = MarketDataPhase::Degraded {
                issue: observation,
                class,
                transport_healthy: 0,
                quoteable: 0,
            };
            return if prior_class != class {
                MarketDataTransition::ClassChanged {
                    from: prior_class,
                    to: class,
                    issue: observation,
                }
            } else {
                MarketDataTransition::Degraded {
                    issue: observation,
                    class,
                }
            };
        }

        let transport_healthy = prior_transport_healthy.saturating_add(1);
        let quoteable = if observation.quoteable() {
            prior_quoteable.saturating_add(1)
        } else {
            0
        };
        if quoteable >= self.coherent_snapshots_to_recover {
            self.phase = MarketDataPhase::Degraded {
                issue: prior_issue,
                class: prior_class,
                transport_healthy,
                quoteable,
            };
            return MarketDataTransition::RecoveryReady;
        }

        if prior_class == MarketDataFaultClass::Transport
            && transport_healthy >= self.coherent_snapshots_to_recover
        {
            let class = MarketDataFaultClass::MarketState;
            let issue = observation
                .fault_class()
                .map_or(prior_issue, |_| observation);
            self.phase = MarketDataPhase::Degraded {
                issue,
                class,
                transport_healthy,
                quoteable,
            };
            return MarketDataTransition::ClassChanged {
                from: prior_class,
                to: class,
                issue: observation,
            };
        }

        let issue = observation
            .fault_class()
            .map_or(prior_issue, |_| observation);
        self.phase = MarketDataPhase::Degraded {
            issue,
            class: prior_class,
            transport_healthy,
            quoteable,
        };
        MarketDataTransition::Recovering {
            class: prior_class,
            transport_healthy,
            quoteable,
            required: self.coherent_snapshots_to_recover,
        }
    }

    /// Confirms recovery only after the effect executor has independently
    /// verified the venue maker book is empty and the latest snapshot remains
    /// quoteable. Until then the phase stays degraded and the strategy gate
    /// prevents every market-data-dependent order.
    pub fn confirm_recovered(&mut self) -> MarketDataTransition {
        if self.recovery_ready() {
            self.phase = MarketDataPhase::Healthy;
            MarketDataTransition::Recovered
        } else {
            match self.phase {
                MarketDataPhase::Degraded { issue, class, .. } => {
                    MarketDataTransition::Degraded { issue, class }
                }
                MarketDataPhase::Healthy | MarketDataPhase::Grace { .. } => {
                    MarketDataTransition::Healthy
                }
            }
        }
    }

    fn begin_bad(
        &mut self,
        now_ms: u64,
        observation: MarketDataObservation,
    ) -> MarketDataTransition {
        if observation == MarketDataObservation::FeedIdle
            || (self.bad_observations_to_degrade == 1 && self.bad_grace_ms == 0)
        {
            let class = observation
                .fault_class()
                .expect("non-coherent observations have a fault class");
            self.phase = MarketDataPhase::Degraded {
                issue: observation,
                class,
                transport_healthy: 0,
                quoteable: 0,
            };
            return MarketDataTransition::EnteredDegraded {
                issue: observation,
                class,
                consecutive: 1,
                bad_for_ms: 0,
            };
        }
        self.phase = MarketDataPhase::Grace {
            first_bad_ms: now_ms,
            consecutive: 1,
            issue: observation,
        };
        MarketDataTransition::Grace {
            issue: observation,
            consecutive: 1,
            bad_for_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_have_expected_fault_classes() {
        assert_eq!(MarketDataObservation::Coherent.fault_class(), None);
        assert_eq!(
            MarketDataObservation::MarkMidDivergence.fault_class(),
            Some(MarketDataFaultClass::MarketState)
        );
        assert_eq!(
            MarketDataObservation::CrossedBook.fault_class(),
            Some(MarketDataFaultClass::MarketState)
        );
        for observation in [
            MarketDataObservation::FeedIdle,
            MarketDataObservation::RestFallback,
            MarketDataObservation::InvalidSnapshot,
        ] {
            assert_eq!(
                observation.fault_class(),
                Some(MarketDataFaultClass::Transport)
            );
        }
    }

    #[test]
    fn rapid_divergence_does_not_bypass_grace() {
        let mut health = MarketDataHealth::default();
        let _ = health.observe(0, MarketDataObservation::MarkMidDivergence);
        let _ = health.observe(100, MarketDataObservation::MarkMidDivergence);
        assert!(matches!(
            health.observe(251, MarketDataObservation::MarkMidDivergence),
            MarketDataTransition::Grace {
                bad_for_ms: 251,
                ..
            }
        ));
        assert!(!health.is_degraded());
    }

    #[test]
    fn sustained_divergence_enters_market_state_after_grace() {
        let mut health = MarketDataHealth::default();
        let _ = health.observe(0, MarketDataObservation::MarkMidDivergence);
        let _ = health.observe(1_000, MarketDataObservation::MarkMidDivergence);
        assert!(matches!(
            health.observe(
                MARKET_DATA_BAD_GRACE_MS,
                MarketDataObservation::MarkMidDivergence,
            ),
            MarketDataTransition::EnteredDegraded {
                class: MarketDataFaultClass::MarketState,
                ..
            }
        ));
    }

    #[test]
    fn market_state_escalates_to_transport_immediately() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::MarkMidDivergence);
        assert_eq!(
            health.observe(1, MarketDataObservation::InvalidSnapshot),
            MarketDataTransition::ClassChanged {
                from: MarketDataFaultClass::MarketState,
                to: MarketDataFaultClass::Transport,
                issue: MarketDataObservation::InvalidSnapshot,
            }
        );
    }

    #[test]
    fn transport_recovers_to_market_state_after_three_structural_samples() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::RestFallback);
        for now_ms in [1, 2] {
            assert!(matches!(
                health.observe(now_ms, MarketDataObservation::MarkMidDivergence),
                MarketDataTransition::Recovering {
                    class: MarketDataFaultClass::Transport,
                    ..
                }
            ));
        }
        assert!(matches!(
            health.observe(3, MarketDataObservation::MarkMidDivergence),
            MarketDataTransition::ClassChanged {
                from: MarketDataFaultClass::Transport,
                to: MarketDataFaultClass::MarketState,
                ..
            }
        ));
        assert_eq!(
            health.degraded_class(),
            Some(MarketDataFaultClass::MarketState)
        );
    }

    #[test]
    fn three_quoteable_samples_require_explicit_confirmation() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::MarkMidDivergence);
        for now_ms in [1, 2] {
            assert!(matches!(
                health.observe(now_ms, MarketDataObservation::Coherent),
                MarketDataTransition::Recovering { .. }
            ));
        }
        assert_eq!(
            health.observe(3, MarketDataObservation::Coherent),
            MarketDataTransition::RecoveryReady
        );
        assert!(health.is_degraded());
        assert_eq!(health.confirm_recovered(), MarketDataTransition::Recovered);
        assert!(!health.is_degraded());
    }

    #[test]
    fn market_state_resets_quoteable_streak_without_becoming_transport() {
        let mut health = MarketDataHealth::new(1, 0, 3);
        let _ = health.observe(0, MarketDataObservation::CrossedBook);
        let _ = health.observe(1, MarketDataObservation::Coherent);
        let _ = health.observe(2, MarketDataObservation::Coherent);
        assert!(matches!(
            health.observe(3, MarketDataObservation::MarkMidDivergence),
            MarketDataTransition::Recovering {
                class: MarketDataFaultClass::MarketState,
                quoteable: 0,
                ..
            }
        ));
    }

    #[test]
    fn feed_idle_degrades_immediately() {
        let mut health = MarketDataHealth::default();
        assert!(matches!(
            health.observe(1, MarketDataObservation::FeedIdle),
            MarketDataTransition::EnteredDegraded {
                class: MarketDataFaultClass::Transport,
                ..
            }
        ));
    }
}
