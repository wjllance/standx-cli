//! External-price defensive guard (stage 3 v1 combined candidate).
//!
//! When a leading external market (Hyperliquid midPx) has already moved and
//! StandX's mark has not yet followed, resting quotes on one side are stale
//! and about to be sniped. The guard temporarily suppresses that endangered
//! side; it releases as soon as the divergence closes (StandX catches up,
//! measured median ~2.6s), so activation is event-scoped seconds — unlike the
//! rejected stage-3 v0 whose position-scoped latch pinned suppression for
//! hours.
//!
//! Failure direction is OPEN: a missing, stale, or non-finite external sample
//! deactivates the guard and quoting continues normally. The external feed is
//! a defensive optimization, never a stop condition — it must not become a new
//! outage source. This module is pure decision logic: no I/O, no clocks; the
//! caller normalizes feed freshness into [`ExternalDivergence::age_ms`].

use standx_sdk::models::OrderSide;
use std::error::Error;
use std::fmt;

/// Operator configuration for the external-price guard (`[external_guard]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuardConfig {
    pub enabled: bool,
    /// Activate when |divergence| reaches this many bps.
    pub enter_bps: f64,
    /// Release when |divergence| falls below this many bps (enter > exit > 0
    /// gives a small anti-flap hysteresis; divergence closes by itself when
    /// StandX's mark catches up, so there is no long-latch risk).
    pub exit_bps: f64,
    /// Samples older than this are treated as absent (fail-open).
    pub max_age_ms: u64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            enter_bps: 6.0,
            exit_bps: 3.0,
            max_age_ms: 5000,
        }
    }
}

/// One caller-normalized external observation for the current cycle.
///
/// `divergence_bps = (leader_mid / standx_mark - 1) × 1e4`: positive means the
/// external price is above StandX's mark (our asks are stale-cheap), negative
/// means below (our bids are stale-rich).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExternalDivergence {
    pub divergence_bps: f64,
    /// Age of the external sample when this cycle's decision is made.
    pub age_ms: u64,
}

/// Per-cycle guard outcome consumed by the planner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuardDecision {
    pub enabled: bool,
    pub active: bool,
    /// The side whose resting quotes are endangered and must not quote.
    pub endangered: Option<OrderSide>,
    /// The divergence the decision was made on (telemetry only).
    pub divergence_bps: Option<f64>,
}

impl GuardDecision {
    pub const INACTIVE: Self = Self {
        enabled: false,
        active: false,
        endangered: None,
        divergence_bps: None,
    };
}

impl Default for GuardDecision {
    fn default() -> Self {
        Self::INACTIVE
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuardError(String);

impl GuardError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for GuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for GuardError {}

/// Stateful enter/exit hysteresis around the divergence signal.
#[derive(Clone, Debug)]
pub struct GuardController {
    config: GuardConfig,
    endangered: Option<OrderSide>,
}

impl GuardController {
    pub fn new(config: GuardConfig) -> Result<Self, GuardError> {
        if !config.enter_bps.is_finite() || !config.exit_bps.is_finite() {
            return Err(GuardError::new("guard thresholds must be finite"));
        }
        if config.exit_bps <= 0.0 || config.exit_bps >= config.enter_bps {
            return Err(GuardError::new(
                "guard thresholds must satisfy 0 < exit_bps < enter_bps",
            ));
        }
        if config.max_age_ms == 0 {
            return Err(GuardError::new("guard max_age_ms must be positive"));
        }
        Ok(Self {
            config,
            endangered: None,
        })
    }

    pub fn config(&self) -> GuardConfig {
        self.config
    }

    /// Currently suppressed side, if any (telemetry/transition logging).
    pub fn endangered(&self) -> Option<OrderSide> {
        self.endangered
    }

    /// Fold one cycle's external observation into the guard state.
    ///
    /// `None`, a stale sample, or a non-finite divergence always deactivates
    /// (fail-open); state never survives a data gap, so a reconnecting feed
    /// starts from a clean slate.
    pub fn observe(&mut self, input: Option<ExternalDivergence>) -> GuardDecision {
        if !self.config.enabled {
            self.endangered = None;
            return GuardDecision::INACTIVE;
        }

        let usable = input.filter(|sample| {
            sample.age_ms <= self.config.max_age_ms && sample.divergence_bps.is_finite()
        });
        let Some(sample) = usable else {
            self.endangered = None;
            return GuardDecision {
                enabled: true,
                active: false,
                endangered: None,
                divergence_bps: None,
            };
        };

        let divergence = sample.divergence_bps;
        let magnitude = divergence.abs();
        // Leader above our mark -> our asks are stale-cheap -> protect Sell.
        let toward = if divergence > 0.0 {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        self.endangered = match self.endangered {
            // Entering, or an active guard whose divergence flipped sign past
            // the enter threshold, points at the side the signal names now.
            _ if magnitude >= self.config.enter_bps => Some(toward),
            // Between exit and enter: hold the previous side (anti-flap).
            Some(side) if magnitude >= self.config.exit_bps => Some(side),
            _ => None,
        };

        GuardDecision {
            enabled: true,
            active: self.endangered.is_some(),
            endangered: self.endangered,
            divergence_bps: Some(divergence),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_config() -> GuardConfig {
        GuardConfig {
            enabled: true,
            ..GuardConfig::default()
        }
    }

    fn fresh(divergence_bps: f64) -> Option<ExternalDivergence> {
        Some(ExternalDivergence {
            divergence_bps,
            age_ms: 100,
        })
    }

    #[test]
    fn rejects_invalid_thresholds() {
        for config in [
            GuardConfig {
                enter_bps: f64::NAN,
                ..enabled_config()
            },
            GuardConfig {
                exit_bps: f64::INFINITY,
                ..enabled_config()
            },
            GuardConfig {
                exit_bps: 0.0,
                ..enabled_config()
            },
            GuardConfig {
                enter_bps: 3.0,
                exit_bps: 3.0,
                ..enabled_config()
            },
            GuardConfig {
                max_age_ms: 0,
                ..enabled_config()
            },
        ] {
            assert!(GuardController::new(config).is_err());
        }
    }

    #[test]
    fn disabled_is_inactive_regardless_of_signal() {
        let mut controller = GuardController::new(GuardConfig::default()).unwrap();
        assert_eq!(controller.observe(fresh(50.0)), GuardDecision::INACTIVE);
        assert_eq!(controller.observe(fresh(-50.0)), GuardDecision::INACTIVE);
    }

    #[test]
    fn direction_maps_to_endangered_side() {
        let mut controller = GuardController::new(enabled_config()).unwrap();
        let up = controller.observe(fresh(8.0));
        assert!(up.active);
        assert_eq!(up.endangered, Some(OrderSide::Sell));

        let mut controller = GuardController::new(enabled_config()).unwrap();
        let down = controller.observe(fresh(-8.0));
        assert!(down.active);
        assert_eq!(down.endangered, Some(OrderSide::Buy));
    }

    #[test]
    fn hysteresis_holds_between_exit_and_enter_and_releases_below_exit() {
        let mut controller = GuardController::new(enabled_config()).unwrap();
        assert!(!controller.observe(fresh(5.9)).active);
        assert!(controller.observe(fresh(6.0)).active);
        // Between exit (3) and enter (6): held.
        let held = controller.observe(fresh(4.0));
        assert!(held.active);
        assert_eq!(held.endangered, Some(OrderSide::Sell));
        // Below exit: released.
        assert!(!controller.observe(fresh(2.9)).active);
        // Re-entry requires the full enter threshold again.
        assert!(!controller.observe(fresh(4.0)).active);
    }

    #[test]
    fn sign_flip_past_enter_switches_sides_immediately() {
        let mut controller = GuardController::new(enabled_config()).unwrap();
        assert_eq!(
            controller.observe(fresh(7.0)).endangered,
            Some(OrderSide::Sell)
        );
        let flipped = controller.observe(fresh(-7.0));
        assert!(flipped.active);
        assert_eq!(flipped.endangered, Some(OrderSide::Buy));
    }

    #[test]
    fn missing_stale_or_nonfinite_samples_fail_open() {
        let mut controller = GuardController::new(enabled_config()).unwrap();
        assert!(controller.observe(fresh(10.0)).active);

        // Missing sample: deactivate.
        let missing = controller.observe(None);
        assert!(!missing.active);
        assert_eq!(missing.divergence_bps, None);
        assert!(missing.enabled);

        // Stale sample: deactivate even though the value is large.
        assert!(controller.observe(fresh(10.0)).active);
        let stale = controller.observe(Some(ExternalDivergence {
            divergence_bps: 10.0,
            age_ms: 5001,
        }));
        assert!(!stale.active);

        // Non-finite: deactivate.
        assert!(controller.observe(fresh(10.0)).active);
        assert!(!controller.observe(fresh(f64::NAN)).active);

        // State never survives a gap: held zone after a gap does not re-latch.
        assert!(!controller.observe(fresh(4.0)).active);
    }

    #[test]
    fn boundary_age_is_still_usable() {
        let mut controller = GuardController::new(enabled_config()).unwrap();
        let decision = controller.observe(Some(ExternalDivergence {
            divergence_bps: 9.0,
            age_ms: 5000,
        }));
        assert!(decision.active);
    }
}
