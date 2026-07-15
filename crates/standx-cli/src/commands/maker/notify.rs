use crate::cli::{AlertWebhookFormat, OutputFormat};
use standx_maker::{Alert, PositionAlertAnchor, PositionRiskKind};
use std::time::Duration;

/// Number of extra attempts after the first POST fails or returns a 5xx.
const WEBHOOK_RETRIES: u32 = 2;

pub(super) fn webhook_body(
    format: AlertWebhookFormat,
    text: &str,
    raw: &serde_json::Value,
) -> serde_json::Value {
    match format {
        AlertWebhookFormat::Slack | AlertWebhookFormat::Telegram => {
            serde_json::json!({ "text": text })
        }
        AlertWebhookFormat::Feishu => serde_json::json!({
            "msg_type": "text",
            "content": { "text": text },
        }),
        AlertWebhookFormat::Raw => raw.clone(),
    }
}

async fn post_webhook(client: reqwest::Client, url: String, body: serde_json::Value) {
    // A single POST drops the alert on a transient 5xx or timeout, so retry a
    // couple of times with linear backoff before giving up.
    for attempt in 0..=WEBHOOK_RETRIES {
        match client
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return,
            Ok(response) => {
                let status = response.status();
                if attempt == WEBHOOK_RETRIES {
                    eprintln!("⚠️  maker webhook returned {status}");
                    return;
                }
                eprintln!(
                    "⚠️  maker webhook returned {status}; retrying ({}/{})",
                    attempt + 1,
                    WEBHOOK_RETRIES
                );
            }
            Err(error) => {
                if attempt == WEBHOOK_RETRIES {
                    eprintln!("⚠️  maker webhook POST failed: {error}");
                    return;
                }
                eprintln!(
                    "⚠️  maker webhook POST failed: {error}; retrying ({}/{})",
                    attempt + 1,
                    WEBHOOK_RETRIES
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(u64::from(attempt) + 1)).await;
    }
}

/// Stable machine-readable name and delivery severity for a position-risk kind.
///
/// `Jump` keeps its historical `position_jump` name for backward compatibility;
/// the direction flip and max-position crossings escalate to `critical` so a
/// reversal or breach is distinguishable from an ordinary threshold jump.
fn risk_kind_descriptor(kind: PositionRiskKind) -> (&'static str, &'static str) {
    match kind {
        PositionRiskKind::Jump => ("position_jump", "warning"),
        PositionRiskKind::DirectionFlip => ("direction_flip", "critical"),
        PositionRiskKind::MaxPositionCrossed => ("max_position_crossed", "critical"),
        PositionRiskKind::InventoryExitCrossed => ("inventory_exit_crossed", "warning"),
    }
}

#[derive(Clone)]
pub(super) struct MakerNotifier {
    output_format: OutputFormat,
    http: Option<reqwest::Client>,
    webhook_url: Option<String>,
    webhook_format: AlertWebhookFormat,
}

impl MakerNotifier {
    pub(super) fn new(
        output_format: OutputFormat,
        webhook_url: Option<String>,
        webhook_format: AlertWebhookFormat,
    ) -> Self {
        let http = webhook_url.as_ref().map(|_| reqwest::Client::new());
        Self {
            output_format,
            http,
            webhook_url,
            webhook_format,
        }
    }

    pub(super) async fn lifecycle(
        &self,
        event: &str,
        text: &str,
        symbol: &str,
        await_delivery: bool,
    ) {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        if self.output_format == OutputFormat::Json {
            println!(
                "{}",
                serde_json::json!({
                    "ts": ts, "symbol": symbol, "action": "lifecycle",
                    "event": event, "message": text,
                })
            );
        }
        let raw = serde_json::json!({
            "text": text, "ts": ts, "symbol": symbol,
            "action": "lifecycle", "event": event,
        });
        self.deliver(text, raw, await_delivery).await;
    }

    pub(super) async fn risk(&self, notice: RiskNotice<'_>, await_delivery: bool) {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let delta = notice
            .position_before
            .zip(notice.position_after)
            .map(|(before, after)| after - before);
        // Give the webhook the same `[severity/kind] symbol:` prefix the stderr
        // line carries so a phone alert identifies its source across instances.
        let text = format!(
            "[{}/{}] {}: {}",
            notice.severity, notice.kind, notice.symbol, notice.message
        );
        let raw = serde_json::json!({
            "text": text,
            "ts": ts,
            "symbol": notice.symbol,
            "cycle": notice.cycle,
            "action": "risk_notification",
            "kind": notice.kind,
            "severity": notice.severity,
            "event": notice.event,
            "message": notice.message,
            "position_before": notice.position_before,
            "position_after": notice.position_after,
            "position_delta": delta,
            "expected_position": notice.expected,
            "observed_position": notice.observed,
        });
        if self.output_format == OutputFormat::Json {
            println!("{raw}");
        } else {
            eprintln!(
                "⚠️  risk [{}/{}] {}: {}",
                notice.severity, notice.kind, notice.symbol, notice.message
            );
        }
        self.deliver(&text, raw, await_delivery).await;
    }

    pub(super) async fn request_timeout(
        &self,
        notice: RequestTimeoutNotice<'_>,
        await_delivery: bool,
    ) {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (text, raw) = request_timeout_payload(&notice, &ts);
        if self.output_format == OutputFormat::Json {
            println!("{raw}");
        } else {
            eprintln!(
                "⚠️  risk [warning/order_request_timeout] {}: {}",
                notice.symbol, notice.message
            );
        }
        self.deliver(&text, raw, await_delivery).await;
    }

    pub(super) async fn position_jump(
        &self,
        anchor: &mut PositionAlertAnchor,
        change: PositionChange<'_>,
    ) {
        let Some(event) = anchor.evaluate(
            change.observed,
            change.max_position,
            change.inventory_exit_pct,
            change.qty_tolerance,
        ) else {
            return;
        };
        let before = event.before;
        let after = event.after;
        let delta = event.delta;
        let (kind, severity) = risk_kind_descriptor(event.kind);
        let attribution = if (change.observed - change.expected).abs() <= change.qty_tolerance {
            "current-run maker fills"
        } else {
            "unreconciled"
        };
        let message = format!(
            "position changed {before:+.8} → {after:+.8} (delta {delta:+.8}, {attribution})"
        );
        self.risk(
            RiskNotice {
                kind,
                severity,
                event: "detected",
                message: &message,
                symbol: change.symbol,
                cycle: change.cycle,
                position_before: Some(before),
                position_after: Some(after),
                expected: Some(change.expected),
                observed: Some(change.observed),
            },
            false,
        )
        .await;
    }

    pub(super) async fn alert(&self, alert: &Alert, symbol: &str, await_delivery: bool) {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let label = if alert.firing {
            "🚨 ALERT"
        } else {
            "✅ RESOLVED"
        };
        if self.output_format == OutputFormat::Json {
            println!(
                "{}",
                serde_json::json!({
                    "ts": ts, "symbol": symbol, "action": "alert",
                    "kind": alert.kind, "firing": alert.firing,
                    "message": alert.message,
                })
            );
        } else {
            eprintln!("{} [{}] {} — {}", label, alert.kind, symbol, alert.message);
        }
        let text = format!("{} [{}] {} — {}", label, symbol, alert.kind, alert.message);
        let raw = serde_json::json!({
            "text": text, "ts": ts, "symbol": symbol, "action": "alert",
            "kind": alert.kind, "firing": alert.firing, "message": alert.message,
        });
        self.deliver(&text, raw, await_delivery).await;
    }

    async fn deliver(&self, text: &str, raw: serde_json::Value, await_delivery: bool) {
        let (Some(client), Some(url)) = (&self.http, &self.webhook_url) else {
            return;
        };
        let body = webhook_body(self.webhook_format, text, &raw);
        if await_delivery {
            post_webhook(client.clone(), url.clone(), body).await;
        } else {
            tokio::spawn(post_webhook(client.clone(), url.clone(), body));
        }
    }
}

/// Severity band for the maker's JWT remaining-lifetime monitor.
///
/// Token lifetime is a hard cap on run duration: once the JWT expires every
/// reconnect and REST call is rejected and the bot halts. There is no renewal
/// endpoint in the codebase, so the best we can do is warn early enough for an
/// operator to re-authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TokenExpiryLevel {
    Ok,
    Warning,
    Critical,
}

/// Classify remaining token lifetime into a severity band. Thresholds are in
/// seconds; `critical_below` should be <= `warn_below`.
pub(super) fn token_expiry_level(
    remaining_secs: i64,
    warn_below: i64,
    critical_below: i64,
) -> TokenExpiryLevel {
    if remaining_secs <= critical_below {
        TokenExpiryLevel::Critical
    } else if remaining_secs <= warn_below {
        TokenExpiryLevel::Warning
    } else {
        TokenExpiryLevel::Ok
    }
}

pub(super) struct RiskNotice<'a> {
    pub(super) kind: &'a str,
    pub(super) severity: &'a str,
    pub(super) event: &'a str,
    pub(super) message: &'a str,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
    pub(super) position_before: Option<f64>,
    pub(super) position_after: Option<f64>,
    pub(super) expected: Option<f64>,
    pub(super) observed: Option<f64>,
}

pub(super) struct RequestTimeoutNotice<'a> {
    pub(super) message: &'a str,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
    pub(super) request_id: &'a str,
    pub(super) request_kind: &'a str,
    pub(super) timeout_phase: &'a str,
    pub(super) age_ms: u64,
    pub(super) timeout_ms: u64,
    pub(super) recovery_target: &'a str,
    pub(super) expected_position: f64,
}

fn request_timeout_payload(
    notice: &RequestTimeoutNotice<'_>,
    ts: &str,
) -> (String, serde_json::Value) {
    let text = format!(
        "[warning/order_request_timeout] {}: {}",
        notice.symbol, notice.message
    );
    let raw = serde_json::json!({
        "text": text,
        "ts": ts,
        "symbol": notice.symbol,
        "cycle": notice.cycle,
        "action": "risk_notification",
        "kind": "order_request_timeout",
        "severity": "warning",
        "event": "frozen",
        "message": notice.message,
        "request_id": notice.request_id,
        "request_kind": notice.request_kind,
        "timeout_phase": notice.timeout_phase,
        "age_ms": notice.age_ms,
        "timeout_ms": notice.timeout_ms,
        "recovery_target": notice.recovery_target,
        "position_before": null,
        "position_after": null,
        "position_delta": null,
        "expected_position": notice.expected_position,
        "observed_position": null,
    });
    (text, raw)
}

pub(super) struct PositionChange<'a> {
    pub(super) observed: f64,
    pub(super) expected: f64,
    pub(super) max_position: f64,
    pub(super) inventory_exit_pct: f64,
    pub(super) qty_tolerance: f64,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_kind_descriptor_escalates_flip_and_breach_to_critical() {
        // Ordinary jump keeps its historical name and warning severity.
        assert_eq!(
            risk_kind_descriptor(PositionRiskKind::Jump),
            ("position_jump", "warning")
        );
        // A reversal and a max-position breach must be distinguishable and
        // escalated so they are not lost among routine jumps.
        assert_eq!(
            risk_kind_descriptor(PositionRiskKind::DirectionFlip),
            ("direction_flip", "critical")
        );
        assert_eq!(
            risk_kind_descriptor(PositionRiskKind::MaxPositionCrossed),
            ("max_position_crossed", "critical")
        );
        assert_eq!(
            risk_kind_descriptor(PositionRiskKind::InventoryExitCrossed),
            ("inventory_exit_crossed", "warning")
        );
    }

    #[test]
    fn request_timeout_payload_preserves_correlation_and_recovery_fields() {
        let notice = RequestTimeoutNotice {
            message: "request timed out",
            symbol: "XAG-USD",
            cycle: 754,
            request_id: "request-7",
            request_kind: "place",
            timeout_phase: "account_order",
            age_ms: 10_250,
            timeout_ms: 10_000,
            recovery_target: "account_stream",
            expected_position: 0.0,
        };
        let (_, raw) = request_timeout_payload(&notice, "2026-07-15T07:38:39Z");

        assert_eq!(raw["action"], "risk_notification");
        assert_eq!(raw["kind"], "order_request_timeout");
        assert_eq!(raw["request_id"], "request-7");
        assert_eq!(raw["request_kind"], "place");
        assert_eq!(raw["timeout_phase"], "account_order");
        assert_eq!(raw["age_ms"], 10_250);
        assert_eq!(raw["timeout_ms"], 10_000);
        assert_eq!(raw["recovery_target"], "account_stream");
    }
}

#[cfg(test)]
mod token_expiry_tests {
    use super::*;

    #[test]
    fn classifies_remaining_lifetime_into_bands() {
        let warn = 2 * 60 * 60; // 2h
        let critical = 15 * 60; // 15m
        assert_eq!(
            token_expiry_level(6 * 60 * 60, warn, critical),
            TokenExpiryLevel::Ok
        );
        assert_eq!(
            token_expiry_level(warn, warn, critical),
            TokenExpiryLevel::Warning
        );
        assert_eq!(
            token_expiry_level(30 * 60, warn, critical),
            TokenExpiryLevel::Warning
        );
        assert_eq!(
            token_expiry_level(critical, warn, critical),
            TokenExpiryLevel::Critical
        );
        assert_eq!(
            token_expiry_level(0, warn, critical),
            TokenExpiryLevel::Critical
        );
    }

    #[test]
    fn severity_bands_are_ordered_for_escalation_checks() {
        assert!(TokenExpiryLevel::Critical > TokenExpiryLevel::Warning);
        assert!(TokenExpiryLevel::Warning > TokenExpiryLevel::Ok);
    }
}
