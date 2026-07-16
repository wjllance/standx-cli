use super::feed::{fresh_ws_sample, FeedState};
use super::recovery::cancel_maker_orders_with_retry;
use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker as maker;
use standx_sdk::client::StandXClient;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, RwLock};

pub(super) const MARKET_DATA_RECOVERY_TIMEOUT: Duration = Duration::from_secs(60);

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

pub(super) fn degradation_detail(
    transition: maker::MarketDataTransition,
    observation_detail: &str,
) -> Option<String> {
    match transition {
        maker::MarketDataTransition::EnteredDegraded {
            issue,
            consecutive,
            bad_for_ms,
        } => Some(format!(
            "market data degraded: issue={} consecutive={} bad_for_ms={bad_for_ms}; {observation_detail}",
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
) -> Option<String> {
    let (observation, detail) = if acquired.source != "ws" {
        let reason = acquired.fallback_reason.unwrap_or("ws_unavailable");
        (
            maker::MarketDataObservation::RestFallback,
            format!("websocket snapshot unavailable; REST fallback reason={reason}"),
        )
    } else if let Some(maker::CycleSkip::MarkMidDivergence { divergence_bps }) = maker::touch_skip(
        acquired.mark,
        acquired.best_bid,
        acquired.best_ask,
        acquired.max_divergence_bps,
    ) {
        (
            maker::MarketDataObservation::MarkMidDivergence,
            format!(
                "mark/mid divergence {divergence_bps:.2}bps exceeds {:.2}bps",
                acquired.max_divergence_bps,
            ),
        )
    } else {
        (
            maker::MarketDataObservation::Coherent,
            "coherent websocket snapshot".to_string(),
        )
    };
    degradation_detail(health.observe(acquired.now_ms, observation), &detail)
}

fn recovery_snapshot_observation(
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
) -> maker::MarketDataObservation {
    let (Some(_), Some(_)) = (best_bid, best_ask) else {
        return maker::MarketDataObservation::InvalidSnapshot;
    };
    match maker::touch_skip(mark, best_bid, best_ask, max_divergence_bps) {
        Some(maker::CycleSkip::MarkMidDivergence { .. }) => {
            maker::MarketDataObservation::MarkMidDivergence
        }
        Some(_) => maker::MarketDataObservation::InvalidSnapshot,
        None => maker::MarketDataObservation::Coherent,
    }
}

async fn wait_until_recovery_ready(
    feed: &Arc<RwLock<FeedState>>,
    updates: &mut watch::Receiver<u64>,
    health: &mut maker::MarketDataHealth,
    health_started: Instant,
    max_divergence_bps: f64,
) -> Result<()> {
    let mut previous = {
        let state = feed.read().await;
        fresh_ws_sample(&state).map(|(_, _, _, version)| version)
    };
    loop {
        updates
            .changed()
            .await
            .map_err(|_| anyhow::anyhow!("market feed task ended during recovery"))?;
        let sample = {
            let state = feed.read().await;
            fresh_ws_sample(&state)
        };
        let Some((mark, best_bid, best_ask, version)) = sample else {
            let now_ms = duration_ms(health_started.elapsed());
            let _ = health.observe(now_ms, maker::MarketDataObservation::InvalidSnapshot);
            continue;
        };
        if !version.both_advanced_from(previous) {
            continue;
        }
        previous = Some(version);
        let observation =
            recovery_snapshot_observation(mark, best_bid, best_ask, max_divergence_bps);
        let now_ms = duration_ms(health_started.elapsed());
        if matches!(
            health.observe(now_ms, observation),
            maker::MarketDataTransition::RecoveryReady
        ) {
            return Ok(());
        }
    }
}

/// Wait for distinct coherent snapshots, then verify the venue book again.
/// The pure health state stays degraded until both requirements pass.
pub(super) struct MarketDataRecovery<'a> {
    pub(super) client: &'a StandXClient,
    pub(super) symbol: &'a str,
    pub(super) output_format: OutputFormat,
    pub(super) live: bool,
    pub(super) feed: Option<&'a Arc<RwLock<FeedState>>>,
    pub(super) updates: Option<&'a mut watch::Receiver<u64>>,
    pub(super) health: &'a mut maker::MarketDataHealth,
    pub(super) health_started: Instant,
    pub(super) max_divergence_bps: f64,
}

pub(super) async fn recover_market_data(recovery: MarketDataRecovery<'_>) -> Result<()> {
    let feed = recovery
        .feed
        .ok_or_else(|| anyhow::anyhow!("market feed unavailable during recovery"))?;
    let updates = recovery
        .updates
        .ok_or_else(|| anyhow::anyhow!("market feed updates unavailable during recovery"))?;
    loop {
        wait_until_recovery_ready(
            feed,
            updates,
            recovery.health,
            recovery.health_started,
            recovery.max_divergence_bps,
        )
        .await?;

        // Cleanup already ran on entry. Verify again after the snapshot streak
        // so an aborted late placement cannot survive recovery.
        if recovery.live {
            cancel_maker_orders_with_retry(
                recovery.client,
                recovery.symbol,
                3,
                recovery.output_format,
            )
            .await?;
        }

        let latest_is_safe = {
            let state = feed.read().await;
            fresh_ws_sample(&state).is_some_and(|(mark, best_bid, best_ask, _)| {
                recovery_snapshot_observation(mark, best_bid, best_ask, recovery.max_divergence_bps)
                    == maker::MarketDataObservation::Coherent
            })
        };
        if latest_is_safe
            && matches!(
                recovery.health.confirm_recovered(),
                maker::MarketDataTransition::Recovered
            )
        {
            return Ok(());
        }
        let now_ms = duration_ms(recovery.health_started.elapsed());
        let _ = recovery
            .health
            .observe(now_ms, maker::MarketDataObservation::InvalidSnapshot);
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_rest_fallback_enters_degraded() {
        let mut health = maker::MarketDataHealth::default();
        for now_ms in [0, 1_000] {
            assert!(observe_acquired_market_health(
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
            )
            .is_none());
        }
        let detail = observe_acquired_market_health(
            &mut health,
            AcquiredMarketHealth {
                now_ms: 2_000,
                source: "rest",
                fallback_reason: Some("ws_mark_and_book_stale"),
                mark: 100.0,
                best_bid: Some(99.9),
                best_ask: Some(100.1),
                max_divergence_bps: 10.0,
            },
        )
        .expect("third consecutive REST fallback must degrade");
        assert!(detail.contains("issue=rest_fallback"));
        assert!(detail.contains("consecutive=3"));
        assert!(health.is_degraded());
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
        assert!(observe_acquired_market_health(
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
        )
        .is_none());
        assert!(!health.is_degraded());
    }

    #[test]
    fn recovery_snapshot_requires_full_non_divergent_touch() {
        assert_eq!(
            recovery_snapshot_observation(100.0, Some(99.9), Some(100.1), 10.0),
            maker::MarketDataObservation::Coherent
        );
        assert_eq!(
            recovery_snapshot_observation(100.0, Some(100.2), Some(100.3), 10.0),
            maker::MarketDataObservation::MarkMidDivergence
        );
        assert_eq!(
            recovery_snapshot_observation(100.0, Some(99.9), None, 10.0),
            maker::MarketDataObservation::InvalidSnapshot
        );
    }
}
