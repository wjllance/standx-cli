//! Pure order-command lifecycle latency accounting.
//!
//! All timestamps are caller-normalized monotonic milliseconds since one
//! process-local origin. UTC milliseconds are retained only for correlation;
//! durations never use wall-clock time.

use standx_sdk::models::OrderSide;
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LatencyRequestKind {
    Place,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyRequestOutcome {
    Accepted,
    Rejected,
    Effective,
    Timeout,
    Invalidated,
    ProcessEnded,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LatencyRequestContext {
    pub request_id: String,
    pub kind: LatencyRequestKind,
    pub generation: u64,
    pub cycle: u64,
    pub symbol: String,
    pub side: Option<OrderSide>,
    pub level: Option<u32>,
    pub order_id: Option<u64>,
    pub market_source: Option<String>,
    pub recovery: bool,
    pub intent_ms: u64,
    pub intent_utc_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LatencyRequest {
    pub context: LatencyRequestContext,
    pub written_ms: Option<u64>,
    pub ack_ms: Option<u64>,
    pub effective_ms: Option<u64>,
    pub outcome: Option<LatencyRequestOutcome>,
    pub fill_after_cancel_ms: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LatencyError {
    DuplicateRequest {
        request_id: String,
    },
    UnknownRequest {
        request_id: String,
    },
    TimeBeforeIntent {
        request_id: String,
        intent_ms: u64,
        event_ms: u64,
    },
    DuplicateStage {
        request_id: String,
        stage: &'static str,
    },
    InvalidTransition {
        request_id: String,
        detail: &'static str,
    },
}

impl fmt::Display for LatencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRequest { request_id } => {
                write!(formatter, "duplicate latency request {request_id}")
            }
            Self::UnknownRequest { request_id } => {
                write!(formatter, "unknown latency request {request_id}")
            }
            Self::TimeBeforeIntent {
                request_id,
                intent_ms,
                event_ms,
            } => write!(
                formatter,
                "latency event for {request_id} at {event_ms} precedes intent at {intent_ms}"
            ),
            Self::DuplicateStage { request_id, stage } => {
                write!(
                    formatter,
                    "request {request_id} has duplicate {stage} stage"
                )
            }
            Self::InvalidTransition { request_id, detail } => {
                write!(
                    formatter,
                    "request {request_id} has invalid transition: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for LatencyError {}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatencyMetricSummary {
    pub samples: u64,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub p99_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LatencySummary {
    pub kind: LatencyRequestKind,
    pub requests: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub effective: u64,
    pub timeout: u64,
    pub invalidated: u64,
    pub process_ended: u64,
    pub pending: u64,
    pub reject_rate: f64,
    pub timeout_rate: f64,
    pub write: LatencyMetricSummary,
    pub ack: LatencyMetricSummary,
    pub effective_latency: LatencyMetricSummary,
    pub fill_after_cancel: LatencyMetricSummary,
}

#[derive(Clone, Debug, Default)]
pub struct OrderLatencyTracker {
    requests: HashMap<String, LatencyRequest>,
    order: Vec<String>,
}

impl OrderLatencyTracker {
    pub fn register(&mut self, context: LatencyRequestContext) -> Result<(), LatencyError> {
        if self.requests.contains_key(&context.request_id) {
            return Err(LatencyError::DuplicateRequest {
                request_id: context.request_id,
            });
        }
        self.order.push(context.request_id.clone());
        self.requests.insert(
            context.request_id.clone(),
            LatencyRequest {
                context,
                written_ms: None,
                ack_ms: None,
                effective_ms: None,
                outcome: None,
                fill_after_cancel_ms: Vec::new(),
            },
        );
        Ok(())
    }

    pub fn mark_written(&mut self, request_id: &str, at_ms: u64) -> Result<(), LatencyError> {
        let request = self.request_mut(request_id, at_ms)?;
        if request.written_ms.is_some() {
            return Err(duplicate_stage(request_id, "write"));
        }
        request.written_ms = Some(at_ms);
        Ok(())
    }

    pub fn mark_ack(
        &mut self,
        request_id: &str,
        at_ms: u64,
        accepted: bool,
    ) -> Result<(), LatencyError> {
        let request = self.request_mut(request_id, at_ms)?;
        if request.ack_ms.is_some() {
            return Err(duplicate_stage(request_id, "ack"));
        }
        if !accepted {
            if request.effective_ms.is_some() {
                return Err(LatencyError::InvalidTransition {
                    request_id: request_id.to_string(),
                    detail: "rejection observed after request became effective",
                });
            }
            request.outcome = Some(LatencyRequestOutcome::Rejected);
        }
        request.ack_ms = Some(at_ms);
        Ok(())
    }

    /// Effective may be observed before the command ack; both timestamps are
    /// still durations from intent and are retained independently.
    pub fn mark_effective(&mut self, request_id: &str, at_ms: u64) -> Result<(), LatencyError> {
        let request = self.request_mut(request_id, at_ms)?;
        if request.effective_ms.is_some() {
            return Err(duplicate_stage(request_id, "effective"));
        }
        match request.outcome {
            Some(LatencyRequestOutcome::Rejected) => {
                return Err(LatencyError::InvalidTransition {
                    request_id: request_id.to_string(),
                    detail: "rejected request cannot become effective",
                });
            }
            Some(
                LatencyRequestOutcome::Timeout
                | LatencyRequestOutcome::Invalidated
                | LatencyRequestOutcome::ProcessEnded,
            ) => {
                // Preserve the censored terminal category while retaining the
                // late effective duration for incident correlation.
                request.effective_ms = Some(at_ms);
                return Ok(());
            }
            _ => {}
        }
        request.effective_ms = Some(at_ms);
        request.outcome = Some(LatencyRequestOutcome::Effective);
        Ok(())
    }

    pub fn mark_timeout(&mut self, request_id: &str, at_ms: u64) -> Result<(), LatencyError> {
        self.mark_terminal(request_id, at_ms, LatencyRequestOutcome::Timeout)
    }

    pub fn mark_invalidated(&mut self, request_id: &str, at_ms: u64) -> Result<(), LatencyError> {
        self.mark_terminal(request_id, at_ms, LatencyRequestOutcome::Invalidated)
    }

    /// Censor every unresolved request whose monotonic observation window has
    /// elapsed. This is deliberately independent of maker cycle count and does
    /// not release any projected venue exposure.
    pub fn timeout_pending(&mut self, at_ms: u64, timeout_ms: u64) -> Result<usize, LatencyError> {
        let request_ids = self
            .requests()
            .filter(|request| {
                request.outcome.is_none()
                    && at_ms.saturating_sub(request.context.intent_ms) >= timeout_ms
            })
            .map(|request| request.context.request_id.clone())
            .collect::<Vec<_>>();
        for request_id in &request_ids {
            self.mark_timeout(request_id, at_ms)?;
        }
        Ok(request_ids.len())
    }

    pub fn record_fill_after_cancel(
        &mut self,
        request_id: &str,
        fill_ms: u64,
    ) -> Result<(), LatencyError> {
        let request = self.request_mut(request_id, fill_ms)?;
        if request.context.kind != LatencyRequestKind::Cancel {
            return Err(LatencyError::InvalidTransition {
                request_id: request_id.to_string(),
                detail: "fill-after-cancel metric requires a cancel request",
            });
        }
        request
            .fill_after_cancel_ms
            .push(fill_ms - request.context.intent_ms);
        Ok(())
    }

    /// Correlate a current-run fill to every cancel intent for its venue order.
    /// Returns the number of matched cancel requests.
    pub fn record_fill_after_cancel_order(
        &mut self,
        order_id: u64,
        fill_ms: u64,
    ) -> Result<usize, LatencyError> {
        let request_ids = self
            .requests()
            .filter(|request| {
                request.context.kind == LatencyRequestKind::Cancel
                    && request.context.order_id == Some(order_id)
            })
            .map(|request| request.context.request_id.clone())
            .collect::<Vec<_>>();
        for request_id in &request_ids {
            self.record_fill_after_cancel(request_id, fill_ms)?;
        }
        Ok(request_ids.len())
    }

    /// Mark unresolved cancel requests effective when an authoritative REST
    /// audit confirms their venue order is absent. Rejected cancels are not
    /// reclassified merely because the order is absent for another reason.
    pub fn mark_absent_cancels_effective(
        &mut self,
        open_order_ids: &[u64],
        at_ms: u64,
    ) -> Result<usize, LatencyError> {
        let request_ids = self
            .requests()
            .filter(|request| {
                request.context.kind == LatencyRequestKind::Cancel
                    && request.effective_ms.is_none()
                    && request.outcome != Some(LatencyRequestOutcome::Rejected)
                    && request
                        .context
                        .order_id
                        .is_some_and(|order_id| !open_order_ids.contains(&order_id))
            })
            .map(|request| request.context.request_id.clone())
            .collect::<Vec<_>>();
        for request_id in &request_ids {
            self.mark_effective(request_id, at_ms)?;
        }
        Ok(request_ids.len())
    }

    /// Classify every remaining request at process end. An accepted ack with
    /// no effective observation remains explicitly `Accepted`; an unacked
    /// request becomes `ProcessEnded` rather than disappearing.
    pub fn finish_process(&mut self, at_ms: u64) -> Result<(), LatencyError> {
        for request_id in self.order.clone() {
            let request = self.request_mut(&request_id, at_ms)?;
            if request.outcome.is_none() {
                request.outcome = Some(if request.ack_ms.is_some() {
                    LatencyRequestOutcome::Accepted
                } else {
                    LatencyRequestOutcome::ProcessEnded
                });
            }
        }
        Ok(())
    }

    pub fn requests(&self) -> impl Iterator<Item = &LatencyRequest> {
        self.order
            .iter()
            .filter_map(|request_id| self.requests.get(request_id))
    }

    pub fn summary(&self, kind: LatencyRequestKind) -> LatencySummary {
        let requests = self
            .requests()
            .filter(|request| request.context.kind == kind)
            .collect::<Vec<_>>();
        let mut write = Vec::new();
        let mut ack = Vec::new();
        let mut effective = Vec::new();
        let mut fill_after_cancel = Vec::new();
        let mut counts = [0_u64; 7];
        for request in &requests {
            let intent = request.context.intent_ms;
            if let Some(at_ms) = request.written_ms {
                write.push(at_ms - intent);
            }
            if let Some(at_ms) = request.ack_ms {
                ack.push(at_ms - request.written_ms.unwrap_or(intent));
            }
            if let Some(at_ms) = request.effective_ms {
                effective.push(at_ms - intent);
            }
            fill_after_cancel.extend(request.fill_after_cancel_ms.iter().copied());
            let index = match request.outcome {
                Some(LatencyRequestOutcome::Accepted) => 0,
                Some(LatencyRequestOutcome::Rejected) => 1,
                Some(LatencyRequestOutcome::Effective) => 2,
                Some(LatencyRequestOutcome::Timeout) => 3,
                Some(LatencyRequestOutcome::Invalidated) => 4,
                Some(LatencyRequestOutcome::ProcessEnded) => 5,
                None => 6,
            };
            counts[index] += 1;
        }
        let total = requests.len() as u64;
        LatencySummary {
            kind,
            requests: total,
            accepted: counts[0],
            rejected: counts[1],
            effective: counts[2],
            timeout: counts[3],
            invalidated: counts[4],
            process_ended: counts[5],
            pending: counts[6],
            reject_rate: rate(counts[1], total),
            timeout_rate: rate(counts[3], total),
            write: metric(write),
            ack: metric(ack),
            effective_latency: metric(effective),
            fill_after_cancel: metric(fill_after_cancel),
        }
    }

    fn mark_terminal(
        &mut self,
        request_id: &str,
        at_ms: u64,
        outcome: LatencyRequestOutcome,
    ) -> Result<(), LatencyError> {
        let request = self.request_mut(request_id, at_ms)?;
        if request.outcome.is_some() {
            return Err(LatencyError::InvalidTransition {
                request_id: request_id.to_string(),
                detail: "request already has a terminal outcome",
            });
        }
        request.outcome = Some(outcome);
        Ok(())
    }

    fn request_mut(
        &mut self,
        request_id: &str,
        event_ms: u64,
    ) -> Result<&mut LatencyRequest, LatencyError> {
        let request =
            self.requests
                .get_mut(request_id)
                .ok_or_else(|| LatencyError::UnknownRequest {
                    request_id: request_id.to_string(),
                })?;
        if event_ms < request.context.intent_ms {
            return Err(LatencyError::TimeBeforeIntent {
                request_id: request_id.to_string(),
                intent_ms: request.context.intent_ms,
                event_ms,
            });
        }
        Ok(request)
    }
}

fn duplicate_stage(request_id: &str, stage: &'static str) -> LatencyError {
    LatencyError::DuplicateStage {
        request_id: request_id.to_string(),
        stage,
    }
}

fn rate(count: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

fn metric(mut values: Vec<u64>) -> LatencyMetricSummary {
    values.sort_unstable();
    LatencyMetricSummary {
        samples: values.len() as u64,
        p50_ms: percentile(&values, 50),
        p95_ms: percentile(&values, 95),
        p99_ms: percentile(&values, 99),
    }
}

fn percentile(values: &[u64], percentile: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let rank = (percentile * values.len()).div_ceil(100);
    values.get(rank.saturating_sub(1)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(
        request_id: &str,
        kind: LatencyRequestKind,
        intent_ms: u64,
    ) -> LatencyRequestContext {
        LatencyRequestContext {
            request_id: request_id.to_string(),
            kind,
            generation: 3,
            cycle: 7,
            symbol: "BTC-USD".to_string(),
            side: Some(OrderSide::Buy),
            level: Some(0),
            order_id: None,
            market_source: Some("ws".to_string()),
            recovery: false,
            intent_ms,
            intent_utc_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn account_effective_before_ack_keeps_both_independent_latencies() {
        let mut tracker = OrderLatencyTracker::default();
        tracker
            .register(context("p1", LatencyRequestKind::Place, 100))
            .unwrap();
        tracker.mark_written("p1", 110).unwrap();
        tracker.mark_effective("p1", 125).unwrap();
        tracker.mark_ack("p1", 140, true).unwrap();
        let summary = tracker.summary(LatencyRequestKind::Place);
        assert_eq!(summary.write.p50_ms, Some(10));
        assert_eq!(summary.ack.p50_ms, Some(30));
        assert_eq!(summary.effective_latency.p50_ms, Some(25));
        assert_eq!(summary.effective, 1);
    }

    #[test]
    fn terminal_categories_cover_every_registered_request() {
        let mut tracker = OrderLatencyTracker::default();
        for id in [
            "accepted",
            "rejected",
            "effective",
            "timeout",
            "invalidated",
            "ended",
        ] {
            tracker
                .register(context(id, LatencyRequestKind::Place, 0))
                .unwrap();
        }
        tracker.mark_ack("accepted", 1, true).unwrap();
        tracker.mark_ack("rejected", 1, false).unwrap();
        tracker.mark_effective("effective", 2).unwrap();
        tracker.mark_timeout("timeout", 3).unwrap();
        tracker.mark_invalidated("invalidated", 4).unwrap();
        tracker.finish_process(5).unwrap();
        let summary = tracker.summary(LatencyRequestKind::Place);
        assert_eq!(summary.requests, 6);
        assert_eq!(summary.accepted, 1);
        assert_eq!(summary.rejected, 1);
        assert_eq!(summary.effective, 1);
        assert_eq!(summary.timeout, 1);
        assert_eq!(summary.invalidated, 1);
        assert_eq!(summary.process_ended, 1);
        assert_eq!(summary.pending, 0);
    }

    #[test]
    fn timeout_samples_remain_visible_beside_success_percentiles() {
        let mut tracker = OrderLatencyTracker::default();
        for (index, latency) in [10, 20, 30, 40].into_iter().enumerate() {
            let id = format!("c{index}");
            tracker
                .register(context(&id, LatencyRequestKind::Cancel, 100))
                .unwrap();
            tracker.mark_written(&id, 100 + latency).unwrap();
            if index == 3 {
                tracker.mark_timeout(&id, 200).unwrap();
            } else {
                tracker.mark_effective(&id, 100 + latency + 5).unwrap();
            }
        }
        tracker.record_fill_after_cancel("c0", 150).unwrap();
        let summary = tracker.summary(LatencyRequestKind::Cancel);
        assert_eq!(summary.write.samples, 4);
        assert_eq!(summary.write.p50_ms, Some(20));
        assert_eq!(summary.write.p95_ms, Some(40));
        assert_eq!(summary.timeout, 1);
        assert!((summary.timeout_rate - 0.25).abs() < 1e-12);
        assert_eq!(summary.fill_after_cancel.p50_ms, Some(50));
    }

    #[test]
    fn monotonic_timeout_censors_latency_without_cycle_expiry() {
        let mut tracker = OrderLatencyTracker::default();
        tracker
            .register(context("old", LatencyRequestKind::Place, 100))
            .unwrap();
        tracker.mark_written("old", 110).unwrap();
        tracker.mark_ack("old", 120, true).unwrap();
        tracker
            .register(context("recent", LatencyRequestKind::Place, 180))
            .unwrap();

        assert_eq!(tracker.timeout_pending(200, 50).unwrap(), 1);
        assert_eq!(tracker.timeout_pending(220, 50).unwrap(), 0);
        let outcomes = tracker
            .requests()
            .map(|request| (request.context.request_id.as_str(), request.outcome))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes,
            vec![
                ("old", Some(LatencyRequestOutcome::Timeout)),
                ("recent", None),
            ]
        );
    }

    #[test]
    fn rest_absence_marks_cancel_effective_but_not_rejected_cancel() {
        let mut tracker = OrderLatencyTracker::default();
        let mut accepted = context("accepted", LatencyRequestKind::Cancel, 10);
        accepted.order_id = Some(42);
        tracker.register(accepted).unwrap();
        tracker.mark_written("accepted", 11).unwrap();
        tracker.mark_ack("accepted", 12, true).unwrap();

        let mut rejected = context("rejected", LatencyRequestKind::Cancel, 10);
        rejected.order_id = Some(43);
        tracker.register(rejected).unwrap();
        tracker.mark_written("rejected", 11).unwrap();
        tracker.mark_ack("rejected", 12, false).unwrap();

        assert_eq!(tracker.mark_absent_cancels_effective(&[], 20).unwrap(), 1);
        assert_eq!(
            tracker
                .requests()
                .find(|request| request.context.request_id == "accepted")
                .and_then(|request| request.effective_ms),
            Some(20)
        );
        assert_eq!(
            tracker
                .requests()
                .find(|request| request.context.request_id == "rejected")
                .and_then(|request| request.effective_ms),
            None
        );
    }

    #[test]
    fn invalid_timing_and_rejected_then_effective_fail() {
        let mut tracker = OrderLatencyTracker::default();
        tracker
            .register(context("p1", LatencyRequestKind::Place, 100))
            .unwrap();
        assert!(matches!(
            tracker.mark_written("p1", 99),
            Err(LatencyError::TimeBeforeIntent { .. })
        ));
        tracker.mark_ack("p1", 110, false).unwrap();
        assert!(matches!(
            tracker.mark_effective("p1", 120),
            Err(LatencyError::InvalidTransition { .. })
        ));
    }
}
