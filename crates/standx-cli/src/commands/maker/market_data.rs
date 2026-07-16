use standx_maker as maker;
use std::time::Duration;

pub(super) const MARKET_DATA_STANDBY_HEARTBEAT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub(super) struct MarketDataDegradedError {
    pub(super) detail: String,
}

impl std::fmt::Display for MarketDataDegradedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for MarketDataDegradedError {}

#[derive(Debug)]
pub(super) struct ClassifiedMarketHealth {
    pub(super) observation: maker::MarketDataObservation,
    pub(super) detail: String,
    pub(super) divergence_bps: Option<f64>,
}

pub(super) fn classify_market_health(
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
) -> ClassifiedMarketHealth {
    let (Some(bid), Some(ask)) = (best_bid, best_ask) else {
        return ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::InvalidSnapshot,
            detail: "websocket snapshot is missing a valid two-sided touch".to_string(),
            divergence_bps: None,
        };
    };

    match maker::touch_skip(mark, Some(bid), Some(ask), max_divergence_bps) {
        Some(maker::CycleSkip::MarkMidDivergence { divergence_bps }) => ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::MarkMidDivergence,
            detail: format!(
                "mark/mid divergence {divergence_bps:.2}bps exceeds {max_divergence_bps:.2}bps"
            ),
            divergence_bps: Some(divergence_bps),
        },
        Some(maker::CycleSkip::CrossedBook) => ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::CrossedBook,
            detail: format!("crossed websocket book: bid={bid:.8} ask={ask:.8}"),
            divergence_bps: None,
        },
        Some(maker::CycleSkip::MissingTouch) => ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::InvalidSnapshot,
            detail: "websocket snapshot has an invalid touch".to_string(),
            divergence_bps: None,
        },
        None => ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::Coherent,
            detail: "coherent websocket snapshot".to_string(),
            divergence_bps: None,
        },
    }
}

#[derive(Debug)]
pub(super) struct MarketHealthUpdate {
    pub(super) transition: maker::MarketDataTransition,
    pub(super) detail: String,
    pub(super) divergence_bps: Option<f64>,
}

impl MarketHealthUpdate {
    pub(super) fn degradation_detail(&self) -> Option<String> {
        degradation_detail(self.transition, &self.detail)
    }
}

pub(super) fn degradation_detail(
    transition: maker::MarketDataTransition,
    observation_detail: &str,
) -> Option<String> {
    match transition {
        maker::MarketDataTransition::EnteredDegraded {
            issue,
            class,
            consecutive,
            bad_for_ms,
        } => Some(format!(
            "market data degraded: class={} issue={} consecutive={} bad_for_ms={bad_for_ms}; {observation_detail}",
            class.label(),
            issue.label(),
            consecutive,
        )),
        _ => None,
    }
}

pub(super) struct AcquiredMarketHealth<'a> {
    pub(super) now_ms: u64,
    pub(super) source: &'a str,
    pub(super) fallback_reason: Option<&'a str>,
    pub(super) mark: f64,
    pub(super) best_bid: Option<f64>,
    pub(super) best_ask: Option<f64>,
    pub(super) max_divergence_bps: f64,
}

pub(super) fn observe_acquired_market_health(
    health: &mut maker::MarketDataHealth,
    acquired: AcquiredMarketHealth<'_>,
) -> MarketHealthUpdate {
    let classified = if acquired.source != "ws" {
        let reason = acquired.fallback_reason.unwrap_or("ws_unavailable");
        ClassifiedMarketHealth {
            observation: maker::MarketDataObservation::RestFallback,
            detail: format!("websocket snapshot unavailable; REST fallback reason={reason}"),
            divergence_bps: None,
        }
    } else {
        classify_market_health(
            acquired.mark,
            acquired.best_bid,
            acquired.best_ask,
            acquired.max_divergence_bps,
        )
    };
    MarketHealthUpdate {
        transition: health.observe(acquired.now_ms, classified.observation),
        detail: classified.detail,
        divergence_bps: classified.divergence_bps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sustained_rest_fallback_enters_transport_degraded_after_grace() {
        let mut health = maker::MarketDataHealth::default();
        for now_ms in [0, 1_000] {
            let update = observe_acquired_market_health(
                &mut health,
                AcquiredMarketHealth {
                    now_ms,
                    source: "rest",
                    fallback_reason: Some("ws_mark_and_book_stale"),
                    mark: 100.0,
                    best_bid: Some(99.9),
                    best_ask: Some(100.1),
                    max_divergence_bps: 10.0,
                },
            );
            assert!(update.degradation_detail().is_none());
        }
        let update = observe_acquired_market_health(
            &mut health,
            AcquiredMarketHealth {
                now_ms: maker::MARKET_DATA_BAD_GRACE_MS,
                source: "rest",
                fallback_reason: Some("ws_mark_and_book_stale"),
                mark: 100.0,
                best_bid: Some(99.9),
                best_ask: Some(100.1),
                max_divergence_bps: 10.0,
            },
        );
        let detail = update
            .degradation_detail()
            .expect("third sustained REST fallback after the grace must degrade");
        assert!(detail.contains("class=transport"));
        assert!(detail.contains("issue=rest_fallback"));
        assert!(detail.contains("consecutive=3"));
    }

    #[test]
    fn coherent_ws_snapshot_clears_grace() {
        let mut health = maker::MarketDataHealth::default();
        let _ = observe_acquired_market_health(
            &mut health,
            AcquiredMarketHealth {
                now_ms: 0,
                source: "rest",
                fallback_reason: Some("ws_book_stale"),
                mark: 100.0,
                best_bid: Some(99.9),
                best_ask: Some(100.1),
                max_divergence_bps: 10.0,
            },
        );
        let update = observe_acquired_market_health(
            &mut health,
            AcquiredMarketHealth {
                now_ms: 1_000,
                source: "ws",
                fallback_reason: None,
                mark: 100.0,
                best_bid: Some(99.9),
                best_ask: Some(100.1),
                max_divergence_bps: 10.0,
            },
        );
        assert_eq!(update.transition, maker::MarketDataTransition::Healthy);
        assert!(!health.is_degraded());
    }

    #[test]
    fn classifier_distinguishes_market_state_from_transport() {
        assert_eq!(
            classify_market_health(100.0, Some(99.9), Some(100.1), 10.0).observation,
            maker::MarketDataObservation::Coherent
        );
        assert_eq!(
            classify_market_health(100.0, Some(100.2), Some(100.3), 10.0).observation,
            maker::MarketDataObservation::MarkMidDivergence
        );
        assert_eq!(
            classify_market_health(100.0, Some(100.1), Some(100.0), 10.0).observation,
            maker::MarketDataObservation::CrossedBook
        );
        assert_eq!(
            classify_market_health(100.0, Some(99.9), None, 10.0).observation,
            maker::MarketDataObservation::InvalidSnapshot
        );
    }
}
