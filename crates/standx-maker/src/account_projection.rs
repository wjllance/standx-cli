//! Deterministic projection of the live maker account stream.
//!
//! The projection deliberately does not account fills. [`crate::MakerLedger`]
//! remains the only fill/PnL/position ingestion path; this module consumes the
//! resulting, already-deduplicated fill outcomes only to keep the projected
//! open quantity in sync.

use crate::{is_current_run_client_order_id, open_qty_adopts, RestingQuote};
use standx_sdk::models::OrderSide;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionPendingPlace {
    pub request_id: String,
    pub client_order_id: String,
    pub side: OrderSide,
    pub price: f64,
    pub qty: f64,
    pub level: u32,
    pub ref_center: f64,
    pub cycle: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionPendingCancel {
    pub request_id: String,
    pub order_id: u64,
    pub side: OrderSide,
    pub level: u32,
    pub price: f64,
    pub cycle: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedOrder {
    pub order_id: u64,
    pub client_order_id: String,
    pub side: OrderSide,
    pub price: f64,
    pub open_qty: f64,
    pub level: u32,
    pub ref_center: f64,
    pub placed_at_cycle: u64,
    total_qty: f64,
    stream_filled_qty: f64,
    ledger_filled_qty: f64,
}

impl ProjectedOrder {
    fn resting_quote(&self) -> RestingQuote {
        RestingQuote {
            order_id: Some(self.order_id.to_string()),
            side: self.side,
            level: self.level,
            price: self.price,
            qty: self.open_qty,
            ref_center: self.ref_center,
            placed_at_cycle: self.placed_at_cycle,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedBalance {
    pub account_type: String,
    pub token: String,
    pub free: String,
    pub total: String,
    pub locked: String,
    pub occupied: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderObservation {
    pub order_id: u64,
    pub client_order_id: Option<String>,
    pub side: OrderSide,
    pub price: f64,
    pub open_qty: f64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountProjectionEvent {
    AdvanceCycle { cycle: u64 },
    PlaceSubmitted(ProjectionPendingPlace),
    PlaceRejected { request_id: String },
    CancelSubmitted(ProjectionPendingCancel),
    CancelResolved { request_id: String },
    OrderObserved(OrderObservation),
    TradeApplied { order_id: u64, qty: f64 },
    PositionObserved { position: f64 },
    BalanceObserved(ProjectedBalance),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionOutcome {
    pub applied: bool,
    pub order_changed: bool,
    pub position_changed: bool,
    pub unknown_current_run_order: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectionMismatch {
    Position {
        projected: f64,
        observed: f64,
    },
    OrderSet {
        projected: Vec<u64>,
        observed: Vec<u64>,
    },
    OrderQuantity {
        order_id: u64,
        projected: f64,
        observed: f64,
    },
}

impl fmt::Display for ProjectionMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Position { projected, observed } => write!(
                formatter,
                "projected position {projected:+.8} differs from REST {observed:+.8}"
            ),
            Self::OrderSet { projected, observed } => write!(
                formatter,
                "projected maker order IDs {projected:?} differ from REST {observed:?}"
            ),
            Self::OrderQuantity {
                order_id,
                projected,
                observed,
            } => write!(
                formatter,
                "projected open qty {projected:.8} for order {order_id} differs from REST {observed:.8}"
            ),
        }
    }
}

impl std::error::Error for ProjectionMismatch {}

#[derive(Debug, Clone)]
pub struct MakerAccountProjection {
    generation: u64,
    run_order_prefix: String,
    orders: HashMap<u64, ProjectedOrder>,
    pending_places: Vec<ProjectionPendingPlace>,
    pending_cancels: Vec<ProjectionPendingCancel>,
    observed_position: f64,
    raw_balances: HashMap<(String, String), ProjectedBalance>,
}

impl MakerAccountProjection {
    pub fn new(generation: u64, run_order_prefix: impl Into<String>, position: f64) -> Self {
        Self {
            generation,
            run_order_prefix: run_order_prefix.into(),
            orders: HashMap::new(),
            pending_places: Vec::new(),
            pending_cancels: Vec::new(),
            observed_position: position,
            raw_balances: HashMap::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn reset(&mut self, generation: u64, position: f64) {
        self.generation = generation;
        self.orders.clear();
        self.pending_places.clear();
        self.pending_cancels.clear();
        self.observed_position = position;
        self.raw_balances.clear();
    }

    pub fn clear_orders_and_pending(&mut self) {
        self.orders.clear();
        self.pending_places.clear();
        self.pending_cancels.clear();
    }

    pub fn observed_position(&self) -> f64 {
        self.observed_position
    }

    pub fn resting_quotes(&self) -> Vec<RestingQuote> {
        let mut orders = self.orders.values().collect::<Vec<_>>();
        orders.sort_by_key(|order| order.order_id);
        orders
            .into_iter()
            .map(ProjectedOrder::resting_quote)
            .collect()
    }

    pub fn pending_places(&self) -> &[ProjectionPendingPlace] {
        &self.pending_places
    }

    pub fn pending_cancels(&self) -> &[ProjectionPendingCancel] {
        &self.pending_cancels
    }

    pub fn raw_balance(&self, account_type: &str, token: &str) -> Option<&ProjectedBalance> {
        self.raw_balances
            .get(&(account_type.to_owned(), token.to_owned()))
    }

    pub fn apply(&mut self, generation: u64, event: AccountProjectionEvent) -> ProjectionOutcome {
        if generation != self.generation {
            return ProjectionOutcome::default();
        }
        match event {
            AccountProjectionEvent::AdvanceCycle { cycle } => {
                self.pending_places
                    .retain(|pending| cycle.saturating_sub(pending.cycle) <= 2);
                self.pending_cancels
                    .retain(|pending| cycle.saturating_sub(pending.cycle) <= 2);
                ProjectionOutcome {
                    applied: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PlaceSubmitted(pending) => {
                self.pending_places.push(pending);
                ProjectionOutcome {
                    applied: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PlaceRejected { request_id } => {
                let before = self.pending_places.len();
                self.pending_places
                    .retain(|pending| pending.request_id != request_id);
                ProjectionOutcome {
                    applied: before != self.pending_places.len(),
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::CancelSubmitted(pending) => {
                self.orders.remove(&pending.order_id);
                self.pending_cancels.push(pending);
                ProjectionOutcome {
                    applied: true,
                    order_changed: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::CancelResolved { request_id } => {
                let Some(index) = self
                    .pending_cancels
                    .iter()
                    .position(|pending| pending.request_id == request_id)
                else {
                    return ProjectionOutcome::default();
                };
                let pending = self.pending_cancels.remove(index);
                let order_changed = self.orders.remove(&pending.order_id).is_some();
                ProjectionOutcome {
                    applied: true,
                    order_changed,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::OrderObserved(observation) => self.observe_order(observation),
            AccountProjectionEvent::TradeApplied { order_id, qty } => {
                let Some(order) = self.orders.get_mut(&order_id) else {
                    return ProjectionOutcome {
                        applied: true,
                        ..ProjectionOutcome::default()
                    };
                };
                order.ledger_filled_qty += qty;
                order.open_qty = (order.total_qty
                    - order.stream_filled_qty.max(order.ledger_filled_qty))
                .max(0.0);
                if order.open_qty <= f64::EPSILON {
                    self.orders.remove(&order_id);
                }
                ProjectionOutcome {
                    applied: true,
                    order_changed: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PositionObserved { position } => {
                let changed = self.observed_position != position;
                self.observed_position = position;
                ProjectionOutcome {
                    applied: true,
                    position_changed: changed,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::BalanceObserved(balance) => {
                let key = (balance.account_type.clone(), balance.token.clone());
                self.raw_balances.insert(key, balance);
                ProjectionOutcome {
                    applied: true,
                    ..ProjectionOutcome::default()
                }
            }
        }
    }

    fn observe_order(&mut self, observation: OrderObservation) -> ProjectionOutcome {
        if !is_current_run_client_order_id(
            observation.client_order_id.as_deref(),
            &self.run_order_prefix,
        ) {
            return ProjectionOutcome::default();
        }
        if observation.terminal || observation.open_qty <= f64::EPSILON {
            let changed = self.orders.remove(&observation.order_id).is_some();
            if let Some(client_order_id) = observation.client_order_id.as_deref() {
                self.pending_places
                    .retain(|pending| pending.client_order_id != client_order_id);
            }
            return ProjectionOutcome {
                applied: true,
                order_changed: changed,
                ..ProjectionOutcome::default()
            };
        }

        let pending_index = observation
            .client_order_id
            .as_deref()
            .and_then(|client_order_id| {
                self.pending_places
                    .iter()
                    .position(|pending| pending.client_order_id == client_order_id)
            })
            .or_else(|| {
                self.pending_places.iter().position(|pending| {
                    pending.side == observation.side
                        && (pending.price - observation.price).abs() <= f64::EPSILON
                        && open_qty_adopts(observation.open_qty, pending.qty)
                })
            });
        let unknown = !self.orders.contains_key(&observation.order_id) && pending_index.is_none();
        let (level, ref_center, placed_at_cycle, total_qty, ledger_filled_qty) = match pending_index
        {
            Some(index) => {
                let pending = self.pending_places.remove(index);
                (
                    pending.level,
                    pending.ref_center,
                    pending.cycle,
                    pending.qty,
                    0.0,
                )
            }
            None => self
                .orders
                .get(&observation.order_id)
                .map(|order| {
                    (
                        order.level,
                        order.ref_center,
                        order.placed_at_cycle,
                        order.total_qty,
                        order.ledger_filled_qty,
                    )
                })
                .unwrap_or((u32::MAX, observation.price, 0, observation.open_qty, 0.0)),
        };
        let stream_filled_qty = (total_qty - observation.open_qty).max(0.0);
        let open_qty = (total_qty - stream_filled_qty.max(ledger_filled_qty)).max(0.0);
        self.orders.insert(
            observation.order_id,
            ProjectedOrder {
                order_id: observation.order_id,
                client_order_id: observation.client_order_id.unwrap_or_default(),
                side: observation.side,
                price: observation.price,
                open_qty,
                level,
                ref_center,
                placed_at_cycle,
                total_qty,
                stream_filled_qty,
                ledger_filled_qty,
            },
        );
        ProjectionOutcome {
            applied: true,
            order_changed: true,
            unknown_current_run_order: unknown,
            ..ProjectionOutcome::default()
        }
    }

    pub fn verify_rest_snapshot(
        &self,
        generation: u64,
        observed_position: f64,
        observed_orders: &[OrderObservation],
        qty_tolerance: f64,
    ) -> Result<(), ProjectionMismatch> {
        if generation != self.generation {
            return Ok(());
        }
        if (self.observed_position - observed_position).abs() > qty_tolerance {
            return Err(ProjectionMismatch::Position {
                projected: self.observed_position,
                observed: observed_position,
            });
        }
        let mut projected = self.orders.keys().copied().collect::<Vec<_>>();
        let mut observed = observed_orders
            .iter()
            .filter(|order| {
                !order.terminal
                    && is_current_run_client_order_id(
                        order.client_order_id.as_deref(),
                        &self.run_order_prefix,
                    )
            })
            .map(|order| order.order_id)
            .collect::<Vec<_>>();
        projected.sort_unstable();
        observed.sort_unstable();
        if projected != observed {
            return Err(ProjectionMismatch::OrderSet {
                projected,
                observed,
            });
        }
        for observation in observed_orders {
            let Some(order) = self.orders.get(&observation.order_id) else {
                continue;
            };
            if (order.open_qty - observation.open_qty).abs() > qty_tolerance {
                return Err(ProjectionMismatch::OrderQuantity {
                    order_id: observation.order_id,
                    projected: order.open_qty,
                    observed: observation.open_qty,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "sxmk-run-";

    fn pending(request_id: &str) -> ProjectionPendingPlace {
        ProjectionPendingPlace {
            request_id: request_id.to_owned(),
            client_order_id: format!("{PREFIX}q00000001b0"),
            side: OrderSide::Buy,
            price: 100.0,
            qty: 0.2,
            level: 0,
            ref_center: 100.0,
            cycle: 1,
        }
    }

    fn order(open_qty: f64, terminal: bool) -> OrderObservation {
        OrderObservation {
            order_id: 7,
            client_order_id: Some(format!("{PREFIX}q00000001b0")),
            side: OrderSide::Buy,
            price: 100.0,
            open_qty,
            terminal,
        }
    }

    #[test]
    fn order_then_trade_and_duplicate_trade_outcome_are_idempotent() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        state.apply(
            1,
            AccountProjectionEvent::TradeApplied {
                order_id: 7,
                qty: 0.1,
            },
        );
        assert_eq!(state.resting_quotes()[0].qty, 0.1);
        // The ledger suppresses duplicate trades, so no second outcome is
        // delivered. Replayed order state converges to the same open qty.
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.1, false)));
        assert_eq!(state.resting_quotes()[0].qty, 0.1);
    }

    #[test]
    fn trade_before_order_does_not_create_phantom_order() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::TradeApplied {
                order_id: 7,
                qty: 0.1,
            },
        );
        assert!(state.resting_quotes().is_empty());
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.1, false)));
        assert_eq!(state.resting_quotes()[0].qty, 0.1);
    }

    #[test]
    fn partial_fill_then_cancel_is_terminal_in_either_order() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        state.apply(
            1,
            AccountProjectionEvent::TradeApplied {
                order_id: 7,
                qty: 0.1,
            },
        );
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        assert!(state.resting_quotes().is_empty());
    }

    #[test]
    fn wrong_run_and_stale_generation_are_ignored() {
        let mut state = MakerAccountProjection::new(2, PREFIX, 0.0);
        let mut wrong = order(0.2, false);
        wrong.client_order_id = Some("sxmk-other-q00000001b0".to_string());
        assert!(
            !state
                .apply(2, AccountProjectionEvent::OrderObserved(wrong))
                .applied
        );
        assert!(
            !state
                .apply(1, AccountProjectionEvent::PlaceSubmitted(pending("old")))
                .applied
        );
        assert!(state.pending_places().is_empty());
    }

    #[test]
    fn cancel_ack_after_close_is_idempotent() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        state.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "c1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 2,
            }),
        );
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        assert!(
            state
                .apply(
                    1,
                    AccountProjectionEvent::CancelResolved {
                        request_id: "c1".to_string()
                    }
                )
                .applied
        );
        assert!(state.resting_quotes().is_empty());
    }

    #[test]
    fn position_and_balance_project_independently_of_ordering() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(
            1,
            AccountProjectionEvent::PositionObserved { position: 0.2 },
        );
        state.apply(
            1,
            AccountProjectionEvent::BalanceObserved(ProjectedBalance {
                account_type: "perps".to_string(),
                token: "DUSD".to_string(),
                free: "90".to_string(),
                total: "100".to_string(),
                locked: "0".to_string(),
                occupied: "10".to_string(),
                updated_at: "now".to_string(),
            }),
        );
        assert_eq!(state.observed_position(), 0.2);
        assert_eq!(state.raw_balance("perps", "DUSD").unwrap().free, "90");
    }

    #[test]
    fn order_before_position_and_position_before_order_converge() {
        let mut order_first = MakerAccountProjection::new(1, PREFIX, 0.0);
        order_first.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        order_first.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        order_first.apply(
            1,
            AccountProjectionEvent::PositionObserved { position: 0.2 },
        );

        let mut position_first = MakerAccountProjection::new(1, PREFIX, 0.0);
        position_first.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        position_first.apply(
            1,
            AccountProjectionEvent::PositionObserved { position: 0.2 },
        );
        position_first.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));

        assert_eq!(
            order_first.observed_position(),
            position_first.observed_position()
        );
        assert_eq!(
            order_first.resting_quotes(),
            position_first.resting_quotes()
        );
    }

    #[test]
    fn rest_audit_detects_order_and_position_drift_without_mutation() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));

        assert!(matches!(
            state.verify_rest_snapshot(1, 0.1, &[order(0.2, false)], 0.001),
            Err(ProjectionMismatch::Position { .. })
        ));
        assert!(matches!(
            state.verify_rest_snapshot(1, 0.0, &[], 0.001),
            Err(ProjectionMismatch::OrderSet { .. })
        ));
        assert_eq!(state.resting_quotes()[0].qty, 0.2);
    }
}
