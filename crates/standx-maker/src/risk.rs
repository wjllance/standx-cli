//! Pure position-risk detection for maker sessions.

/// The reason a position change requires a maker risk notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionRiskKind {
    Jump,
    DirectionFlip,
    MaxPositionCrossed,
    InventoryExitCrossed,
}

/// A position change accepted by the alert anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionRiskEvent {
    pub kind: PositionRiskKind,
    pub before: f64,
    pub after: f64,
    pub delta: f64,
}

/// Deduplicates position-risk alerts while allowing small changes to
/// accumulate until they cross a configured threshold.
#[derive(Debug)]
pub struct PositionAlertAnchor {
    position: f64,
    change_pct: f64,
}

impl PositionAlertAnchor {
    pub fn new(position: f64, change_pct: f64) -> Self {
        Self {
            position,
            change_pct,
        }
    }

    /// Evaluate a position update without sending a notification.
    pub fn evaluate(
        &mut self,
        observed: f64,
        max_position: f64,
        inventory_exit_pct: f64,
        qty_tolerance: f64,
    ) -> Option<PositionRiskEvent> {
        let before = self.position;
        let delta = observed - before;
        if delta.abs() <= qty_tolerance {
            return None;
        }

        let jump_threshold = max_position * self.change_pct / 100.0;
        let jump = self.change_pct > 0.0 && delta.abs() + qty_tolerance >= jump_threshold;
        let direction_flip = before * observed < 0.0;
        let crossed_max = before.abs() + qty_tolerance < max_position
            && observed.abs() + qty_tolerance >= max_position;
        let exit_threshold = max_position * inventory_exit_pct / 100.0;
        let crossed_exit = inventory_exit_pct > 0.0
            && before.abs() + qty_tolerance < exit_threshold
            && observed.abs() + qty_tolerance >= exit_threshold;

        let kind = if direction_flip {
            PositionRiskKind::DirectionFlip
        } else if crossed_max {
            PositionRiskKind::MaxPositionCrossed
        } else if crossed_exit {
            PositionRiskKind::InventoryExitCrossed
        } else if jump {
            PositionRiskKind::Jump
        } else {
            return None;
        };
        self.position = observed;
        Some(PositionRiskEvent {
            kind,
            before,
            after: observed,
            delta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_changes_until_the_jump_threshold() {
        let mut anchor = PositionAlertAnchor::new(0.001, 20.0);
        assert!(anchor.evaluate(0.10, 0.8, 25.0, 0.0005).is_none());
        let event = anchor.evaluate(0.161, 0.8, 25.0, 0.0005).unwrap();
        assert_eq!(event.kind, PositionRiskKind::Jump);
        assert!((event.delta - 0.160).abs() < 1e-9);
        assert!(anchor.evaluate(0.161, 0.8, 25.0, 0.0005).is_none());
    }

    #[test]
    fn prioritizes_direction_and_threshold_crossings() {
        let mut direction = PositionAlertAnchor::new(0.01, 0.0);
        assert_eq!(
            direction.evaluate(-0.01, 0.8, 0.0, 0.0005).unwrap().kind,
            PositionRiskKind::DirectionFlip
        );

        let mut exit = PositionAlertAnchor::new(0.19, 0.0);
        assert_eq!(
            exit.evaluate(0.20, 0.8, 25.0, 0.0005).unwrap().kind,
            PositionRiskKind::InventoryExitCrossed
        );
    }
}
