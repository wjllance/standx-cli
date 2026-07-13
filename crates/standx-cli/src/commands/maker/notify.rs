use crate::cli::{AlertWebhookFormat, OutputFormat};
use standx_maker::{Alert, PositionAlertAnchor};
use std::time::Duration;

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
    match client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if !response.status().is_success() => {
            eprintln!("⚠️  maker webhook returned {}", response.status())
        }
        Err(error) => eprintln!("⚠️  maker webhook POST failed: {error}"),
        _ => {}
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
        let raw = serde_json::json!({
            "text": notice.message,
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
        self.deliver(notice.message, raw, await_delivery).await;
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
                kind: "position_jump",
                severity: "warning",
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

    pub(super) fn alert(&self, alert: &Alert, symbol: &str) {
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
        if let (Some(client), Some(url)) = (&self.http, &self.webhook_url) {
            let text = format!("{} [{}] {} — {}", label, symbol, alert.kind, alert.message);
            let raw = serde_json::json!({
                "text": text, "ts": ts, "symbol": symbol, "action": "alert",
                "kind": alert.kind, "firing": alert.firing, "message": alert.message,
            });
            let body = webhook_body(self.webhook_format, &text, &raw);
            tokio::spawn(post_webhook(client.clone(), url.clone(), body));
        }
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

pub(super) struct PositionChange<'a> {
    pub(super) observed: f64,
    pub(super) expected: f64,
    pub(super) max_position: f64,
    pub(super) inventory_exit_pct: f64,
    pub(super) qty_tolerance: f64,
    pub(super) symbol: &'a str,
    pub(super) cycle: u64,
}
