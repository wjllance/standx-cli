//! Deterministic projection of the live maker account stream.
//!
//! The projection deliberately does not account fills. [`crate::MakerLedger`]
//! remains the only fill/PnL/position ingestion path; this module consumes the
//! resulting, already-deduplicated fill outcomes only to keep the projected
//! open quantity in sync.

use crate::{is_current_run_client_order_id, open_qty_adopts, RestingQuote};
use standx_sdk::models::OrderSide;
use std::collections::{HashMap, VecDeque};
use std::fmt;

pub const MAX_PENDING_ORDER_REQUESTS: usize = 256;

/// Recently cancelled current-run venue order IDs kept to recognize replayed
/// account-stream updates after the cancel request has been accepted. This is
/// deliberately bounded: older observations still fail closed rather than
/// turning a long-lived maker session into an unbounded trust cache.
const MAX_RETIRED_ORDER_IDS: usize = 512;

/// Recently completed command request IDs and their typed request metadata.
/// This keeps duplicate/late acknowledgements idempotent and lets a delayed
/// account-order update recover the exact accepted place after cleanup closes
/// its quote slot. The bound keeps long-running sessions from accumulating
/// unbounded correlation state; older replays continue to fail closed.
const MAX_COMPLETED_ORDER_REQUESTS: usize = 512;

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
pub enum ProjectionPendingRequest {
    Place(ProjectionPendingPlace),
    Cancel(ProjectionPendingCancel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionRequestResolution {
    PlaceAccepted,
    PlaceRejected,
    CancelResolved,
}

/// Whether the order-response (placement) channel survived a verified maker
/// cleanup or was torn down and replaced. This is the *decision* a recovery
/// flow makes; [`MakerAccountProjection::finish_verified_cleanup`] maps it to
/// the mechanical projection reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderResponseContinuity {
    /// The order-response stream is still the same channel, so acknowledgements
    /// it has not yet delivered may still arrive and must stay correlated.
    Preserved,
    /// The order-response stream was replaced, so no acknowledgement for a
    /// request issued on the old channel can ever arrive; end those
    /// obligations as part of the cleanup.
    Replaced,
}

impl ProjectionRequestResolution {
    pub fn accepts_response(self, accepted: bool) -> bool {
        match self {
            Self::PlaceAccepted | Self::CancelResolved => accepted,
            Self::PlaceRejected => !accepted,
        }
    }
}

impl ProjectionPendingRequest {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Place(pending) => &pending.request_id,
            Self::Cancel(pending) => &pending.request_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionRegistryError {
    Capacity { limit: usize },
    DuplicateRequestId { request_id: String },
}

impl fmt::Display for ProjectionRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity { limit } => write!(
                formatter,
                "order-response request registry reached its limit of {limit}"
            ),
            Self::DuplicateRequestId { request_id } => {
                write!(
                    formatter,
                    "duplicate order-response request ID {request_id}"
                )
            }
        }
    }
}

impl std::error::Error for ProjectionRegistryError {}

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
    PlaceAccepted { request_id: String },
    PlaceRejected { request_id: String },
    CancelSubmitted(ProjectionPendingCancel),
    CancelResolved { request_id: String },
    OrderObserved(OrderObservation),
    TradeApplied { order_id: u64, qty: f64 },
    PositionObserved { position: f64 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionOutcome {
    pub applied: bool,
    pub order_changed: bool,
    pub position_changed: bool,
    pub unknown_current_run_order: bool,
    /// Request whose venue-visible order state became effective. This may be
    /// observed before its independent command acknowledgement.
    pub effective_request_id: Option<String>,
    pub request_registry_error: Option<ProjectionRegistryError>,
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

/// One in-flight place/cancel request, tracked in a single registry.
///
/// A request has two independent lifecycles that used to live in two parallel
/// collections:
///
/// - `ack_pending`: still awaiting the command-stream ack (`PlaceAccepted` /
///   `PlaceRejected` / `CancelResolved`). Counts toward the registry capacity
///   and request-id dedup.
/// - `slot_open`: still an unmatched pending place/cancel — visible in the
///   `pending_places()` / `pending_cancels()` views and eligible for order
///   adoption. Cleared only once the order is observed, rejected, resolved,
///   or explicit cleanup invalidates the venue exposure.
///
/// The two clear independently: a place can be adopted from the account stream
/// (slot closes) before its command-stream ack arrives, or observed terminal
/// while a late ack is still outstanding. An entry is dropped only once both
/// are false — see [`MakerAccountProjection::drop_settled`].
#[derive(Debug, Clone, PartialEq)]
struct PendingEntry {
    request: ProjectionPendingRequest,
    ack_pending: bool,
    slot_open: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct CompletedRequest {
    request: ProjectionPendingRequest,
    resolution: ProjectionRequestResolution,
}

impl CompletedRequest {
    fn request_id(&self) -> &str {
        self.request.request_id()
    }

    fn accepted_place(&self) -> Option<&ProjectionPendingPlace> {
        match (&self.request, self.resolution) {
            (
                ProjectionPendingRequest::Place(place),
                ProjectionRequestResolution::PlaceAccepted,
            ) => Some(place),
            _ => None,
        }
    }

    fn resolved_cancel(&self) -> Option<&ProjectionPendingCancel> {
        match (&self.request, self.resolution) {
            (
                ProjectionPendingRequest::Cancel(cancel),
                ProjectionRequestResolution::CancelResolved,
            ) => Some(cancel),
            _ => None,
        }
    }
}

impl PendingEntry {
    fn request_id(&self) -> &str {
        self.request.request_id()
    }

    fn place(&self) -> Option<&ProjectionPendingPlace> {
        match &self.request {
            ProjectionPendingRequest::Place(place) => Some(place),
            ProjectionPendingRequest::Cancel(_) => None,
        }
    }

    fn cancel(&self) -> Option<&ProjectionPendingCancel> {
        match &self.request {
            ProjectionPendingRequest::Cancel(cancel) => Some(cancel),
            ProjectionPendingRequest::Place(_) => None,
        }
    }

    fn is_settled(&self) -> bool {
        !self.ack_pending && !self.slot_open
    }
}

/// Level assigned to a current-run order adopted with neither a matching
/// pending place nor a prior projection (e.g. one observed after a reconnect).
/// It is deliberately outside the maker's real level range so `reconcile`
/// treats it as `Stale` and cancels it, rather than mistaking it for a live
/// quote slot the strategy would try to hold.
const UNKNOWN_ADOPTED_LEVEL: u32 = u32::MAX;

/// The slot metadata adopted for an observed order: where it sits in the quote
/// ladder and how much of it has already filled.
struct AdoptedSlot {
    level: u32,
    ref_center: f64,
    placed_at_cycle: u64,
    total_qty: f64,
    ledger_filled_qty: f64,
}

impl AdoptedSlot {
    fn from_place(place: &ProjectionPendingPlace) -> Self {
        Self {
            level: place.level,
            ref_center: place.ref_center,
            placed_at_cycle: place.cycle,
            total_qty: place.qty,
            ledger_filled_qty: 0.0,
        }
    }

    fn from_existing(order: &ProjectedOrder) -> Self {
        Self {
            level: order.level,
            ref_center: order.ref_center,
            placed_at_cycle: order.placed_at_cycle,
            total_qty: order.total_qty,
            ledger_filled_qty: order.ledger_filled_qty,
        }
    }

    fn unknown(observation: &OrderObservation) -> Self {
        Self {
            level: UNKNOWN_ADOPTED_LEVEL,
            ref_center: observation.price,
            placed_at_cycle: 0,
            total_qty: observation.open_qty,
            ledger_filled_qty: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MakerAccountProjection {
    generation: u64,
    run_order_prefix: String,
    orders: HashMap<u64, ProjectedOrder>,
    pending: Vec<PendingEntry>,
    completed: VecDeque<CompletedRequest>,
    retired_order_ids: VecDeque<u64>,
    observed_position: f64,
    /// Half a price tick. Adopting a venue-echoed order by price must tolerate
    /// the representation difference between the submitted and echoed values
    /// (up to several ULPs at a ~100 price); an exact/EPSILON compare would
    /// miss the pending place it belongs to.
    price_tolerance: f64,
    /// Half a qty tick. Open quantity at or below this is treated as fully
    /// filled (sub-tick dust), not a still-resting order.
    qty_tolerance: f64,
}

impl MakerAccountProjection {
    pub fn new(
        generation: u64,
        run_order_prefix: impl Into<String>,
        position: f64,
        price_tolerance: f64,
        qty_tolerance: f64,
    ) -> Self {
        Self {
            generation,
            run_order_prefix: run_order_prefix.into(),
            orders: HashMap::new(),
            pending: Vec::new(),
            completed: VecDeque::new(),
            retired_order_ids: VecDeque::new(),
            observed_position: position,
            price_tolerance,
            qty_tolerance,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn reset(&mut self, generation: u64, position: f64) {
        self.generation = generation;
        self.orders.clear();
        self.pending.clear();
        self.completed.clear();
        self.retired_order_ids.clear();
        self.observed_position = position;
    }

    /// Begin a new account-stream epoch after maker cleanup without dropping
    /// acknowledgements that are still in flight on the independent
    /// order-response stream. The cleanup has removed executable venue orders,
    /// so quote slots are closed; only correlation metadata and bounded retired
    /// order IDs survive the stream epoch change.
    pub fn reset_after_cleanup_preserving_pending_acks(&mut self, generation: u64, position: f64) {
        self.generation = generation;
        self.clear_orders_preserving_pending_acks();
        self.observed_position = position;
    }

    /// Close every executable quote slot after a maker cleanup has verified
    /// the venue book is empty, resolving in-flight order-response
    /// acknowledgements according to `continuity`. Both variants close the
    /// venue slots — that invariant lives here, in one place — and differ only
    /// in whether pending request correlation survives the cleanup. Recovery
    /// flows call this instead of the mechanical `clear_orders_*` primitives so
    /// the decision they make (did the placement channel survive?) is explicit.
    pub fn finish_verified_cleanup(&mut self, continuity: OrderResponseContinuity) {
        match continuity {
            OrderResponseContinuity::Preserved => self.clear_orders_preserving_pending_acks(),
            OrderResponseContinuity::Replaced => self.clear_orders_and_pending(),
        }
    }

    pub fn clear_orders_and_pending(&mut self) {
        self.orders.clear();
        self.pending.clear();
    }

    /// Clear executable quote state while retaining acknowledgements that the
    /// order-response stream has not delivered yet. A fill or account update
    /// can arrive before its correlated order response; a reconciliation
    /// freeze must therefore close the quote slots without turning that later,
    /// valid response into an unknown request ID.
    pub fn clear_orders_preserving_pending_acks(&mut self) {
        let order_ids = self.orders.keys().copied().collect::<Vec<_>>();
        for order_id in order_ids {
            self.remember_retired_order(order_id);
        }
        self.orders.clear();
        for entry in &mut self.pending {
            entry.slot_open = false;
        }
        self.drop_settled();
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

    /// Open pending places, derived from the registry. Cheap to rebuild — the
    /// set is bounded by the maker's level count and is only ever iterated.
    pub fn pending_places(&self) -> Vec<ProjectionPendingPlace> {
        self.pending
            .iter()
            .filter(|entry| entry.slot_open)
            .filter_map(|entry| entry.place().cloned())
            .collect()
    }

    /// Open pending cancels, derived from the registry.
    pub fn pending_cancels(&self) -> Vec<ProjectionPendingCancel> {
        self.pending
            .iter()
            .filter(|entry| entry.slot_open)
            .filter_map(|entry| entry.cancel().cloned())
            .collect()
    }

    pub fn pending_request(&self, request_id: &str) -> Option<&ProjectionPendingRequest> {
        self.pending
            .iter()
            .find(|entry| entry.ack_pending && entry.request_id() == request_id)
            .map(|entry| &entry.request)
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending
            .iter()
            .filter(|entry| entry.ack_pending)
            .count()
    }

    /// Whether either half of a submitted request lifecycle is still open.
    ///
    /// A request remains live while its acknowledgement or venue-exposure
    /// slot is still open. For an accepted place that means waiting for the
    /// corresponding account-order observation; terminal responses and
    /// explicit cleanup may close a lifecycle earlier. The CLI uses this
    /// clock-free query without duplicating projection correlation rules.
    pub fn has_pending_request_lifecycle(&self, request_id: &str) -> bool {
        self.pending
            .iter()
            .any(|entry| entry.request_id() == request_id)
    }

    pub fn completed_request_resolution(
        &self,
        request_id: &str,
    ) -> Option<ProjectionRequestResolution> {
        self.completed
            .iter()
            .rev()
            .find(|entry| entry.request_id() == request_id)
            .map(|entry| entry.resolution)
    }

    pub fn apply(&mut self, generation: u64, event: AccountProjectionEvent) -> ProjectionOutcome {
        if generation != self.generation {
            return ProjectionOutcome::default();
        }
        match event {
            AccountProjectionEvent::AdvanceCycle { .. } => {
                // Strategy cycles are not a transport deadline: account/order
                // events can advance several cycles inside one wall-clock
                // second. Keep every pending venue exposure reserved until an
                // explicit response, account-order observation, or cleanup
                // closes it. Silently expiring here can permit a duplicate
                // place while the original request is still live.
                ProjectionOutcome {
                    applied: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PlaceSubmitted(pending) => {
                if let Err(error) = self.register_request(ProjectionPendingRequest::Place(pending))
                {
                    return ProjectionOutcome {
                        request_registry_error: Some(error),
                        ..ProjectionOutcome::default()
                    };
                }
                ProjectionOutcome {
                    applied: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PlaceAccepted { request_id } => {
                // The venue accepted the place: it is no longer ack-pending.
                // The slot stays open until the order is observed.
                let request = self
                    .pending
                    .iter_mut()
                    .find(|entry| {
                        entry.ack_pending
                            && entry.request_id() == request_id
                            && entry.place().is_some()
                    })
                    .map(|entry| {
                        entry.ack_pending = false;
                        entry.request.clone()
                    });
                let applied = request.is_some();
                if let Some(request) = request {
                    self.remember_completed_request(
                        request,
                        ProjectionRequestResolution::PlaceAccepted,
                    );
                }
                self.drop_settled();
                ProjectionOutcome {
                    applied,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::PlaceRejected { request_id } => {
                // A reject is terminal: it clears both the ack and the slot.
                let request = self
                    .pending
                    .iter_mut()
                    .find(|entry| entry.request_id() == request_id && entry.place().is_some())
                    .map(|entry| {
                        entry.ack_pending = false;
                        entry.slot_open = false;
                        entry.request.clone()
                    });
                let applied = request.is_some();
                if let Some(request) = request {
                    self.remember_completed_request(
                        request,
                        ProjectionRequestResolution::PlaceRejected,
                    );
                }
                self.drop_settled();
                ProjectionOutcome {
                    applied,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::CancelSubmitted(pending) => {
                let order_id = pending.order_id;
                if let Err(error) = self.register_request(ProjectionPendingRequest::Cancel(pending))
                {
                    return ProjectionOutcome {
                        request_registry_error: Some(error),
                        ..ProjectionOutcome::default()
                    };
                }
                self.orders.remove(&order_id);
                self.remember_retired_order(order_id);
                ProjectionOutcome {
                    applied: true,
                    order_changed: true,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::CancelResolved { request_id } => {
                let index = self
                    .pending
                    .iter()
                    .position(|entry| entry.request_id() == request_id && entry.cancel().is_some());
                let Some(index) = index else {
                    return ProjectionOutcome::default();
                };
                // Only a still-open cancel is holding an order out of the map;
                // cleanup or a terminal account observation may have closed
                // the slot before the response arrives.
                let entry = self.pending.remove(index);
                let order_changed = if entry.slot_open {
                    let order_id = entry.cancel().expect("cancel entry").order_id;
                    self.orders.remove(&order_id).is_some()
                } else {
                    false
                };
                self.remember_completed_request(
                    entry.request,
                    ProjectionRequestResolution::CancelResolved,
                );
                ProjectionOutcome {
                    applied: true,
                    order_changed,
                    ..ProjectionOutcome::default()
                }
            }
            AccountProjectionEvent::OrderObserved(observation) => self.observe_order(observation),
            AccountProjectionEvent::TradeApplied { order_id, qty } => {
                let qty_tolerance = self.qty_tolerance;
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
                if order.open_qty <= qty_tolerance {
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
        }
    }

    fn register_request(
        &mut self,
        request: ProjectionPendingRequest,
    ) -> Result<(), ProjectionRegistryError> {
        if self
            .pending
            .iter()
            .any(|entry| entry.request_id() == request.request_id())
            || self
                .completed_request_resolution(request.request_id())
                .is_some()
        {
            return Err(ProjectionRegistryError::DuplicateRequestId {
                request_id: request.request_id().to_string(),
            });
        }
        if self.pending_request_count() >= MAX_PENDING_ORDER_REQUESTS {
            return Err(ProjectionRegistryError::Capacity {
                limit: MAX_PENDING_ORDER_REQUESTS,
            });
        }
        self.pending.push(PendingEntry {
            request,
            ack_pending: true,
            slot_open: true,
        });
        Ok(())
    }

    /// Drop registry entries whose ack and slot lifecycles have both completed.
    fn drop_settled(&mut self) {
        self.pending.retain(|entry| !entry.is_settled());
    }

    fn remember_completed_request(
        &mut self,
        request: ProjectionPendingRequest,
        resolution: ProjectionRequestResolution,
    ) {
        if self
            .completed
            .iter()
            .any(|entry| entry.request_id() == request.request_id())
        {
            return;
        }
        self.completed.push_back(CompletedRequest {
            request,
            resolution,
        });
        if self.completed.len() > MAX_COMPLETED_ORDER_REQUESTS {
            self.completed.pop_front();
        }
    }

    fn remember_retired_order(&mut self, order_id: u64) {
        if self.retired_order_ids.contains(&order_id) {
            return;
        }
        self.retired_order_ids.push_back(order_id);
        if self.retired_order_ids.len() > MAX_RETIRED_ORDER_IDS {
            self.retired_order_ids.pop_front();
        }
    }

    fn observe_order(&mut self, observation: OrderObservation) -> ProjectionOutcome {
        if !is_current_run_client_order_id(
            observation.client_order_id.as_deref(),
            &self.run_order_prefix,
        ) {
            return ProjectionOutcome::default();
        }
        if observation.terminal || observation.open_qty <= self.qty_tolerance {
            self.handle_terminal_observation(&observation)
        } else {
            self.adopt_open_observation(observation)
        }
    }

    /// A terminal (or zero-qty) observation removes the projected order and
    /// closes matching place/cancel slots — registry entries linger when an
    /// acknowledgement is still pending so the late response can settle.
    fn handle_terminal_observation(&mut self, observation: &OrderObservation) -> ProjectionOutcome {
        let known = self.order_observation_is_known(observation);
        let changed = self.orders.remove(&observation.order_id).is_some();
        let effective_request_id = self
            .pending
            .iter()
            .find(|entry| {
                entry.slot_open
                    && entry
                        .cancel()
                        .is_some_and(|cancel| cancel.order_id == observation.order_id)
            })
            .map(|entry| entry.request_id().to_string())
            .or_else(|| {
                self.completed.iter().rev().find_map(|entry| {
                    entry
                        .resolved_cancel()
                        .filter(|cancel| cancel.order_id == observation.order_id)
                        .map(|_| entry.request_id().to_string())
                })
            })
            .or_else(|| {
                let client_order_id = observation.client_order_id.as_deref()?;
                self.pending
                    .iter()
                    .find(|entry| {
                        entry.slot_open
                            && entry
                                .place()
                                .is_some_and(|place| place.client_order_id == client_order_id)
                    })
                    .map(|entry| entry.request_id().to_string())
            });
        for entry in &mut self.pending {
            let matches = match &entry.request {
                ProjectionPendingRequest::Place(place) => observation
                    .client_order_id
                    .as_deref()
                    .is_some_and(|client_order_id| place.client_order_id == client_order_id),
                ProjectionPendingRequest::Cancel(cancel) => cancel.order_id == observation.order_id,
            };
            if matches {
                entry.slot_open = false;
            }
        }
        self.drop_settled();
        if known {
            self.remember_retired_order(observation.order_id);
        }
        ProjectionOutcome {
            applied: true,
            order_changed: changed,
            effective_request_id,
            ..ProjectionOutcome::default()
        }
    }

    /// A live (non-terminal) observation adopts the order's slot: match it to a
    /// pending place if possible, otherwise fall back to any existing
    /// projection, then to an unknown-order slot, and reconcile open qty.
    fn adopt_open_observation(&mut self, observation: OrderObservation) -> ProjectionOutcome {
        let retired = self.retired_order_ids.contains(&observation.order_id);
        // A replay of an order already cancelled/cleared must remain stale so
        // reconcile cancels it again; completed place metadata must not turn
        // it back into a quote the strategy is willing to hold.
        let pending_match = if retired {
            None
        } else {
            self.match_pending_slot(&observation)
        };
        let completed_match = if retired || pending_match.is_some() {
            None
        } else {
            self.completed_place_slot(&observation)
        };
        let effective_request_id = pending_match
            .as_ref()
            .or(completed_match.as_ref())
            .map(|(_, request_id)| request_id.clone());
        let slot = pending_match
            .map(|(slot, _)| slot)
            .or_else(|| completed_match.map(|(slot, _)| slot));
        let known = slot.is_some() || self.orders.contains_key(&observation.order_id) || retired;
        let slot = slot.unwrap_or_else(|| {
            self.orders
                .get(&observation.order_id)
                .map(AdoptedSlot::from_existing)
                .unwrap_or_else(|| AdoptedSlot::unknown(&observation))
        });
        let stream_filled_qty = (slot.total_qty - observation.open_qty).max(0.0);
        let open_qty = (slot.total_qty - stream_filled_qty.max(slot.ledger_filled_qty)).max(0.0);
        self.orders.insert(
            observation.order_id,
            ProjectedOrder {
                order_id: observation.order_id,
                client_order_id: observation.client_order_id.unwrap_or_default(),
                side: observation.side,
                price: observation.price,
                open_qty,
                level: slot.level,
                ref_center: slot.ref_center,
                placed_at_cycle: slot.placed_at_cycle,
                total_qty: slot.total_qty,
                stream_filled_qty,
                ledger_filled_qty: slot.ledger_filled_qty,
            },
        );
        ProjectionOutcome {
            applied: true,
            order_changed: true,
            unknown_current_run_order: !known,
            effective_request_id,
            ..ProjectionOutcome::default()
        }
    }

    /// Find the open pending place this observation fills — by client-order-id,
    /// else by a side/price/qty heuristic — close its slot, and return the slot
    /// info to adopt. Returns `None` when no pending place matches.
    fn match_pending_slot(
        &mut self,
        observation: &OrderObservation,
    ) -> Option<(AdoptedSlot, String)> {
        let price_tolerance = self.price_tolerance;
        let index = self
            .pending
            .iter()
            .position(|entry| {
                entry.slot_open
                    && entry.place().is_some_and(|place| {
                        Some(place.client_order_id.as_str())
                            == observation.client_order_id.as_deref()
                    })
            })
            .or_else(|| {
                self.pending.iter().position(|entry| {
                    entry.slot_open
                        && entry.place().is_some_and(|place| {
                            place.side == observation.side
                                && (place.price - observation.price).abs() <= price_tolerance
                                && open_qty_adopts(observation.open_qty, place.qty)
                        })
                })
            })?;
        let place = self.pending[index]
            .place()
            .expect("matched entry is a place")
            .clone();
        let request_id = self.pending[index].request_id().to_string();
        self.pending[index].slot_open = false;
        self.drop_settled();
        Some((AdoptedSlot::from_place(&place), request_id))
    }

    fn completed_place_slot(
        &self,
        observation: &OrderObservation,
    ) -> Option<(AdoptedSlot, String)> {
        let client_order_id = observation.client_order_id.as_deref()?;
        self.completed.iter().rev().find_map(|entry| {
            entry
                .accepted_place()
                .filter(|place| place.client_order_id == client_order_id)
                .map(|place| {
                    (
                        AdoptedSlot::from_place(place),
                        entry.request_id().to_string(),
                    )
                })
        })
    }

    fn order_observation_is_known(&self, observation: &OrderObservation) -> bool {
        self.orders.contains_key(&observation.order_id)
            || self.retired_order_ids.contains(&observation.order_id)
            || observation
                .client_order_id
                .as_deref()
                .is_some_and(|client_order_id| {
                    self.pending.iter().any(|entry| {
                        entry
                            .place()
                            .is_some_and(|place| place.client_order_id == client_order_id)
                    }) || self.completed.iter().any(|entry| {
                        entry
                            .accepted_place()
                            .is_some_and(|place| place.client_order_id == client_order_id)
                    })
                })
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
    fn account_order_reports_place_effective_before_ack() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert_eq!(outcome.effective_request_id.as_deref(), Some("p1"));
        assert_eq!(
            state.pending_request("p1"),
            Some(&ProjectionPendingRequest::Place(pending("p1")))
        );
    }

    #[test]
    fn terminal_account_order_reports_cancel_effective() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        assert_eq!(outcome.effective_request_id.as_deref(), Some("c1"));
    }

    #[test]
    fn terminal_account_order_reports_cancel_effective_after_ack() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        state.apply(
            1,
            AccountProjectionEvent::CancelResolved {
                request_id: "c1".to_string(),
            },
        );

        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        assert_eq!(outcome.effective_request_id.as_deref(), Some("c1"));
    }

    #[test]
    fn order_then_trade_and_duplicate_trade_outcome_are_idempotent() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        let mut state = MakerAccountProjection::new(2, PREFIX, 0.0, 0.005, 0.00005);
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
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        assert!(state.pending_cancels().is_empty());
        assert!(matches!(
            state.pending_request("c1"),
            Some(ProjectionPendingRequest::Cancel(_))
        ));
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
    fn late_open_after_cancel_ack_is_recognized_as_a_retired_current_run_order() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::PlaceAccepted {
                request_id: "p1".to_string(),
            },
        );
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
        state.apply(
            1,
            AccountProjectionEvent::CancelResolved {
                request_id: "c1".to_string(),
            },
        );

        // The order channel can replay an open state after the cancel command
        // was accepted. It is still ours, so project it as stale for another
        // cancellation instead of treating it as an external/unknown order.
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert!(outcome.applied && outcome.order_changed);
        assert!(!outcome.unknown_current_run_order);
        assert_eq!(state.resting_quotes()[0].level, UNKNOWN_ADOPTED_LEVEL);
    }

    #[test]
    fn cleanup_marks_cleared_orders_as_retired_for_late_open_replays() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::PlaceAccepted {
                request_id: "p1".to_string(),
            },
        );
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));

        state.clear_orders_preserving_pending_acks();
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert!(outcome.applied && outcome.order_changed);
        assert!(!outcome.unknown_current_run_order);
        assert_eq!(state.resting_quotes()[0].level, UNKNOWN_ADOPTED_LEVEL);
    }

    #[test]
    fn account_reconnect_reset_preserves_unacked_order_response_registry() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "c1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        state.reset_after_cleanup_preserving_pending_acks(2, 0.0);
        assert_eq!(state.generation(), 2);
        assert!(state.pending_places().is_empty());
        assert!(state.pending_cancels().is_empty());
        assert!(matches!(
            state.pending_request("p1"),
            Some(ProjectionPendingRequest::Place(_))
        ));
        assert!(matches!(
            state.pending_request("c1"),
            Some(ProjectionPendingRequest::Cancel(_))
        ));

        state.apply(
            2,
            AccountProjectionEvent::PlaceAccepted {
                request_id: "p1".to_string(),
            },
        );
        state.apply(
            2,
            AccountProjectionEvent::CancelResolved {
                request_id: "c1".to_string(),
            },
        );
        assert_eq!(state.pending_request_count(), 0);
        state.reset_after_cleanup_preserving_pending_acks(3, 0.0);
        assert_eq!(
            state.completed_request_resolution("p1"),
            Some(ProjectionRequestResolution::PlaceAccepted)
        );
        assert_eq!(
            state.completed_request_resolution("c1"),
            Some(ProjectionRequestResolution::CancelResolved)
        );
    }

    #[test]
    fn late_place_ack_matches_after_account_order_is_already_terminal() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(1, AccountProjectionEvent::OrderObserved(order(0.0, true)));
        assert!(state.pending_places().is_empty());
        assert!(matches!(
            state.pending_request("p1"),
            Some(ProjectionPendingRequest::Place(_))
        ));

        let outcome = state.apply(
            1,
            AccountProjectionEvent::PlaceAccepted {
                request_id: "p1".to_string(),
            },
        );
        assert!(outcome.applied);
        assert_eq!(state.pending_request_count(), 0);
    }

    #[test]
    fn freeze_closes_quote_slots_but_preserves_unacked_response_registry() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "c1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        state.clear_orders_preserving_pending_acks();
        assert!(state.pending_places().is_empty());
        assert!(state.pending_cancels().is_empty());
        assert!(matches!(
            state.pending_request("p1"),
            Some(ProjectionPendingRequest::Place(_))
        ));
        assert!(matches!(
            state.pending_request("c1"),
            Some(ProjectionPendingRequest::Cancel(_))
        ));

        assert!(
            state
                .apply(
                    1,
                    AccountProjectionEvent::PlaceAccepted {
                        request_id: "p1".to_string(),
                    },
                )
                .applied
        );
        assert!(
            state
                .apply(
                    1,
                    AccountProjectionEvent::CancelResolved {
                        request_id: "c1".to_string(),
                    },
                )
                .applied
        );
        assert_eq!(state.pending_request_count(), 0);
    }

    #[test]
    fn request_registry_is_strictly_bounded_and_rejects_duplicates() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        for index in 0..MAX_PENDING_ORDER_REQUESTS {
            let outcome = state.apply(
                1,
                AccountProjectionEvent::PlaceSubmitted(pending(&format!("p{index}"))),
            );
            assert!(outcome.request_registry_error.is_none());
        }
        assert_eq!(state.pending_request_count(), MAX_PENDING_ORDER_REQUESTS);

        let overflow = state.apply(
            1,
            AccountProjectionEvent::PlaceSubmitted(pending("overflow")),
        );
        assert!(matches!(
            overflow.request_registry_error,
            Some(ProjectionRegistryError::Capacity {
                limit: MAX_PENDING_ORDER_REQUESTS
            })
        ));

        let mut duplicate = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        duplicate.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("same")));
        let outcome = duplicate.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("same")));
        assert!(matches!(
            outcome.request_registry_error,
            Some(ProjectionRegistryError::DuplicateRequestId { .. })
        ));
    }

    #[test]
    fn position_projects_independently_of_ordering() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        let outcome = state.apply(
            1,
            AccountProjectionEvent::PositionObserved { position: 0.2 },
        );
        assert!(outcome.position_changed);
        assert_eq!(state.observed_position(), 0.2);
    }

    #[test]
    fn order_before_position_and_position_before_order_converge() {
        let mut order_first = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        order_first.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        order_first.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        order_first.apply(
            1,
            AccountProjectionEvent::PositionObserved { position: 0.2 },
        );

        let mut position_first = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
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

    #[test]
    fn rapid_cycle_advances_keep_unconfirmed_slots_reserved() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "c1".to_string(),
                order_id: 9,
                side: OrderSide::Sell,
                level: 0,
                price: 101.0,
                cycle: 1,
            }),
        );
        assert_eq!(state.pending_places().len(), 1);
        assert_eq!(state.pending_cancels().len(), 1);

        // Account events can wake several cycles in one wall-clock second.
        // None may release the quote slot while the original venue request is
        // still awaiting a correlated outcome.
        for cycle in 2..=100 {
            state.apply(1, AccountProjectionEvent::AdvanceCycle { cycle });
        }
        assert_eq!(state.pending_places().len(), 1);
        assert_eq!(state.pending_cancels().len(), 1);
        assert_eq!(state.pending_request_count(), 2);
        assert!(matches!(
            state.pending_request("p1"),
            Some(ProjectionPendingRequest::Place(_))
        ));
        assert!(matches!(
            state.pending_request("c1"),
            Some(ProjectionPendingRequest::Cancel(_))
        ));

        // A rejection is the explicit terminal outcome that releases it.
        state.apply(
            1,
            AccountProjectionEvent::PlaceRejected {
                request_id: "p1".to_string(),
            },
        );
        assert!(state.pending_places().is_empty());
        assert_eq!(state.pending_request_count(), 1);
        state.apply(
            1,
            AccountProjectionEvent::CancelResolved {
                request_id: "c1".to_string(),
            },
        );
        assert!(state.pending_cancels().is_empty());
        assert_eq!(state.pending_request_count(), 0);
    }

    #[test]
    fn accepted_place_stays_reserved_until_account_order_is_visible() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::PlaceAccepted {
                request_id: "p1".to_string(),
            },
        );
        for cycle in 2..=100 {
            state.apply(1, AccountProjectionEvent::AdvanceCycle { cycle });
        }
        assert_eq!(state.pending_places().len(), 1);
        assert_eq!(state.pending_request_count(), 0);
        assert_eq!(
            state.completed_request_resolution("p1"),
            Some(ProjectionRequestResolution::PlaceAccepted)
        );

        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert!(outcome.applied && outcome.order_changed);
        assert!(
            !outcome.unknown_current_run_order,
            "a delayed account update must retain the accepted place identity"
        );
        assert!(state.pending_places().is_empty());
        assert_eq!(outcome.effective_request_id.as_deref(), Some("p1"));
        assert_eq!(state.resting_quotes()[0].level, 0);
    }

    #[test]
    fn rejected_place_tombstone_does_not_authorize_an_open_order() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));
        state.apply(
            1,
            AccountProjectionEvent::PlaceRejected {
                request_id: "p1".to_string(),
            },
        );

        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert!(outcome.unknown_current_run_order);
    }

    #[test]
    fn completed_request_tombstones_are_bounded_and_reset_with_the_run() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        for index in 0..=MAX_COMPLETED_ORDER_REQUESTS {
            let request_id = format!("p{index}");
            state.apply(
                1,
                AccountProjectionEvent::PlaceSubmitted(pending(&request_id)),
            );
            state.apply(1, AccountProjectionEvent::PlaceRejected { request_id });
        }
        assert_eq!(state.completed.len(), MAX_COMPLETED_ORDER_REQUESTS);
        assert_eq!(state.completed_request_resolution("p0"), None);
        assert_eq!(
            state.completed_request_resolution(&format!("p{MAX_COMPLETED_ORDER_REQUESTS}")),
            Some(ProjectionRequestResolution::PlaceRejected)
        );

        state.reset(2, 0.0);
        assert!(state.completed.is_empty());
    }

    #[test]
    fn open_observation_adopts_pending_by_price_qty_heuristic() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));

        // A different (but still current-run) client-order-id that matches the
        // pending place on side/price/qty is adopted via the heuristic branch.
        let mut observation = order(0.2, false);
        observation.order_id = 42;
        observation.client_order_id = Some(format!("{PREFIX}q00000009z9"));
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(observation));

        assert!(outcome.applied && outcome.order_changed);
        assert!(
            !outcome.unknown_current_run_order,
            "a heuristic pending match is not an unknown order"
        );
        let resting = state.resting_quotes();
        assert_eq!(resting.len(), 1);
        assert_eq!(resting[0].level, 0, "adopts the pending place's level");
        assert!(state.pending_places().is_empty(), "the slot is consumed");
    }

    #[test]
    fn unknown_current_run_order_adopts_with_sentinel_level() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);

        // A current-run order with no pending place and no prior projection is
        // adopted at the out-of-range sentinel level so reconcile cancels it.
        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(order(0.2, false)));
        assert!(outcome.applied);
        assert!(outcome.unknown_current_run_order);
        let resting = state.resting_quotes();
        assert_eq!(resting.len(), 1);
        assert_eq!(resting[0].level, UNKNOWN_ADOPTED_LEVEL);
    }

    #[test]
    fn heuristic_adopts_pending_despite_one_ulp_price_echo_difference() {
        let mut state = MakerAccountProjection::new(1, PREFIX, 0.0, 0.005, 0.00005);
        // pending("p1") rests a buy at price 100.0, qty 0.2, level 0.
        state.apply(1, AccountProjectionEvent::PlaceSubmitted(pending("p1")));

        // The venue echoes the "same" price one ULP away (~1.4e-14 at 100) —
        // far above f64::EPSILON but far below half a price tick. The old
        // `<= f64::EPSILON` compare would miss the pending place and adopt the
        // order at the unknown sentinel level; the tick tolerance matches it.
        let echoed_price = f64::from_bits(100.0_f64.to_bits() + 1);
        assert_ne!(echoed_price, 100.0);
        assert!((echoed_price - 100.0).abs() > f64::EPSILON);

        let mut observation = order(0.2, false);
        observation.order_id = 55;
        // A current-run id that does NOT match the pending's client-order-id,
        // forcing the side/price/qty heuristic branch.
        observation.client_order_id = Some(format!("{PREFIX}q00000042c0"));
        observation.price = echoed_price;

        let outcome = state.apply(1, AccountProjectionEvent::OrderObserved(observation));
        assert!(outcome.applied && outcome.order_changed);
        assert!(
            !outcome.unknown_current_run_order,
            "a one-ULP price echo still matches its pending place"
        );
        assert_eq!(
            state.resting_quotes()[0].level,
            0,
            "adopts the pending place's real level, not the unknown sentinel"
        );
        assert!(state.pending_places().is_empty());
    }
}
