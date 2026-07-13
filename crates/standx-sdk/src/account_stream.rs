//! Authenticated user order, position, and trade notifications.

use crate::auth::Credentials;
use crate::error::{Error, Result};
use crate::models::{deserialize_order_side_optional, OrderSide, OrderStatus};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_ACCOUNT_STREAM_URL: &str = "wss://perps.standx.com/ws-stream/v1";
const ACCOUNT_STREAM_ROTATE_AFTER: Duration = Duration::from_secs(23 * 60 * 60 + 50 * 60);

fn string_or_number<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}

fn u64_string_or_number<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = string_or_number(deserializer)?;
    value.parse::<u64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountChannel {
    Order,
    Position,
    Trade,
}

impl AccountChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Position => "position",
            Self::Trade => "trade",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderUpdate {
    #[serde(default)]
    pub seq: u64,
    #[serde(rename = "id", deserialize_with = "u64_string_or_number")]
    pub order_id: u64,
    #[serde(default)]
    pub cl_ord_id: Option<String>,
    pub symbol: String,
    pub side: OrderSide,
    #[serde(deserialize_with = "string_or_number")]
    pub qty: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub fill_qty: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub fill_avg_price: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub price: String,
    pub status: OrderStatus,
    #[serde(default)]
    pub reduce_only: bool,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PositionUpdate {
    #[serde(default)]
    pub seq: u64,
    #[serde(default, deserialize_with = "u64_string_or_number")]
    pub id: u64,
    pub symbol: String,
    #[serde(default, deserialize_with = "deserialize_order_side_optional")]
    pub side: Option<OrderSide>,
    #[serde(deserialize_with = "string_or_number")]
    pub qty: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub entry_price: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub realized_pnl: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountEvent {
    Connected { epoch: u64 },
    Order(OrderUpdate),
    Position(PositionUpdate),
    TradeShadow { seq: u64, data: serde_json::Value },
    Disconnected { reason: String },
    Error { reason: String },
}

#[derive(Debug, Clone)]
pub struct AccountStreamHealth {
    healthy: Arc<AtomicBool>,
    failure_reason: Arc<Mutex<Option<String>>>,
    epoch: Arc<AtomicU64>,
    last_seq: Arc<AtomicU64>,
}

impl AccountStreamHealth {
    fn new(epoch: u64) -> Self {
        Self {
            healthy: Arc::new(AtomicBool::new(true)),
            failure_reason: Arc::new(Mutex::new(None)),
            epoch: Arc::new(AtomicU64::new(epoch)),
            last_seq: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    pub fn failure_reason(&self) -> Option<String> {
        self.failure_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn last_seq(&self) -> u64 {
        self.last_seq.load(Ordering::Acquire)
    }

    pub fn mark_unhealthy(&self, reason: impl Into<String>) {
        if let Ok(mut failure_reason) = self.failure_reason.lock() {
            *failure_reason = Some(reason.into());
        }
        self.healthy.store(false, Ordering::Release);
    }
}

pub struct AccountStream {
    url: String,
    token: String,
    epoch: u64,
}

impl AccountStream {
    pub fn new(epoch: u64) -> Result<Self> {
        let credentials = Credentials::load()?;
        if credentials.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT".to_string(),
            });
        }
        Ok(Self {
            url: DEFAULT_ACCOUNT_STREAM_URL.to_string(),
            token: credentials.token,
            epoch,
        })
    }

    #[cfg(test)]
    fn with_url_and_token(url: impl Into<String>, token: impl Into<String>, epoch: u64) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            epoch,
        }
    }

    pub async fn connect(
        &self,
        channels: &[AccountChannel],
    ) -> Result<(
        mpsc::Receiver<AccountEvent>,
        AccountStreamHealth,
        tokio::task::JoinHandle<()>,
    )> {
        if channels.is_empty() {
            return Err(Error::Validation {
                field: "channels".to_string(),
                message: "account stream requires at least one channel".to_string(),
            });
        }
        let (stream, _) = connect_async(&self.url).await?;
        let (mut write, mut read) = stream.split();
        let streams = channels
            .iter()
            .map(|channel| serde_json::json!({ "channel": channel.as_str() }))
            .collect::<Vec<_>>();
        write
            .send(Message::Text(
                serde_json::json!({
                    "auth": { "token": self.token, "streams": streams }
                })
                .to_string()
                .into(),
            ))
            .await?;

        loop {
            let message = tokio::time::timeout(Duration::from_secs(10), read.next())
                .await
                .map_err(|_| Error::WebSocket {
                    message: "timed out waiting for account-stream authentication".to_string(),
                })?
                .ok_or_else(|| Error::WebSocket {
                    message: "account stream closed before authentication".to_string(),
                })??;
            match message {
                Message::Text(text) => {
                    let envelope: serde_json::Value = serde_json::from_str(&text)?;
                    if envelope.get("channel").and_then(|value| value.as_str()) != Some("auth") {
                        return Err(Error::WebSocket {
                            message: "unexpected account event before authentication".to_string(),
                        });
                    }
                    let code = envelope
                        .pointer("/data/code")
                        .and_then(|value| value.as_i64())
                        .unwrap_or_default();
                    if code != 200 && code != 0 {
                        return Err(Error::AuthRequired {
                            message: envelope
                                .pointer("/data/msg")
                                .and_then(|value| value.as_str())
                                .unwrap_or("account-stream authentication rejected")
                                .to_string(),
                            resolution: "Run 'standx auth login' and retry".to_string(),
                        });
                    }
                    break;
                }
                Message::Ping(payload) => write.send(Message::Pong(payload)).await?,
                Message::Close(_) => {
                    return Err(Error::WebSocket {
                        message: "account stream closed before authentication".to_string(),
                    });
                }
                _ => {}
            }
        }

        let (tx, rx) = mpsc::channel(512);
        let health = AccountStreamHealth::new(self.epoch);
        let task_health = health.clone();
        let epoch = self.epoch;
        let _ = tx.send(AccountEvent::Connected { epoch }).await;
        let handle = tokio::spawn(async move {
            let rotation = tokio::time::sleep(ACCOUNT_STREAM_ROTATE_AFTER);
            tokio::pin!(rotation);
            loop {
                let message = tokio::select! {
                    _ = &mut rotation => {
                        let reason = "account stream proactive 23h50m rotation".to_string();
                        task_health.mark_unhealthy(reason.clone());
                        let _ = tx.send(AccountEvent::Disconnected { reason }).await;
                        return;
                    }
                    message = read.next() => message,
                };
                let Some(message) = message else {
                    let reason = "account stream ended without a close frame".to_string();
                    task_health.mark_unhealthy(reason.clone());
                    let _ = tx.send(AccountEvent::Disconnected { reason }).await;
                    return;
                };
                match message {
                    Ok(Message::Text(text)) => {
                        let result = parse_account_event(&text, &task_health);
                        match result {
                            Ok(Some(event)) => {
                                if tx.send(event).await.is_err() {
                                    return;
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                let reason = format!("invalid account-stream payload: {error}");
                                task_health.mark_unhealthy(reason.clone());
                                let _ = tx.send(AccountEvent::Error { reason }).await;
                                return;
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if let Err(error) = write.send(Message::Pong(payload)).await {
                            let reason = format!("account-stream pong failed: {error}");
                            task_health.mark_unhealthy(reason.clone());
                            let _ = tx.send(AccountEvent::Error { reason }).await;
                            return;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        let reason = frame.map_or_else(
                            || "account stream closed without a close frame".to_string(),
                            |frame| {
                                format!(
                                    "account stream closed: code={} reason={:?}",
                                    u16::from(frame.code),
                                    frame.reason
                                )
                            },
                        );
                        task_health.mark_unhealthy(reason.clone());
                        let _ = tx.send(AccountEvent::Disconnected { reason }).await;
                        return;
                    }
                    Err(error) => {
                        let reason = format!("account stream WebSocket error: {error}");
                        task_health.mark_unhealthy(reason.clone());
                        let _ = tx.send(AccountEvent::Error { reason }).await;
                        return;
                    }
                    _ => {}
                }
            }
        });
        Ok((rx, health, handle))
    }
}

fn parse_account_event(text: &str, health: &AccountStreamHealth) -> Result<Option<AccountEvent>> {
    let envelope: serde_json::Value = serde_json::from_str(text)?;
    let Some(channel) = envelope.get("channel").and_then(|value| value.as_str()) else {
        return Ok(None);
    };
    if channel == "auth" {
        return Ok(None);
    }
    let seq = envelope
        .get("seq")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| Error::WebSocket {
            message: format!("{channel} event has no numeric seq"),
        })?;
    let previous = health.last_seq.swap(seq, Ordering::AcqRel);
    if previous != 0 && seq <= previous {
        return Err(Error::WebSocket {
            message: format!("account stream seq regressed from {previous} to {seq}"),
        });
    }
    let data = envelope
        .get("data")
        .cloned()
        .ok_or_else(|| Error::WebSocket {
            message: format!("{channel} event has no data"),
        })?;
    match channel {
        "order" => {
            let mut order = serde_json::from_value::<OrderUpdate>(data)?;
            order.seq = seq;
            Ok(Some(AccountEvent::Order(order)))
        }
        "position" => {
            let mut position = serde_json::from_value::<PositionUpdate>(data)?;
            position.seq = seq;
            Ok(Some(AccountEvent::Position(position)))
        }
        "trade" => Ok(Some(AccountEvent::TradeShadow { seq, data })),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::accept_async;

    #[test]
    fn parses_official_order_and_position_shapes() {
        let health = AccountStreamHealth::new(1);
        let order = parse_account_event(
            r#"{"seq":35,"channel":"order","data":{"id":2547027,"cl_ord_id":"sxmk-run-q1b0","symbol":"XAG-USD","side":"buy","qty":"0.200","fill_qty":"0.200","fill_avg_price":"58.87","price":"58.87","status":"filled","reduce_only":false,"updated_at":"2026-07-12T22:10:04Z"}}"#,
            &health,
        )
        .unwrap()
        .unwrap();
        assert!(
            matches!(order, AccountEvent::Order(update) if update.order_id == 2_547_027 && update.seq == 35)
        );

        let position = parse_account_event(
            r#"{"seq":36,"channel":"position","data":{"id":80853,"symbol":"XAG-USD","qty":"0.201","entry_price":"58.88","realized_pnl":"7.21","status":"open","updated_at":"2026-07-12T22:10:04Z"}}"#,
            &health,
        )
        .unwrap()
        .unwrap();
        assert!(
            matches!(position, AccountEvent::Position(update) if update.qty == "0.201" && update.side.is_none() && update.seq == 36)
        );

        let short_position = parse_account_event(
            r#"{"seq":37,"channel":"position","data":{"id":80853,"symbol":"XAG-USD","qty":"-0.116","entry_price":"58.24","realized_pnl":"7.12","status":"open","updated_at":"2026-07-13T05:56:25Z"}}"#,
            &health,
        )
        .unwrap()
        .unwrap();
        assert!(
            matches!(short_position, AccountEvent::Position(update) if update.side.is_none() && update.qty == "-0.116" && update.seq == 37)
        );
    }

    #[test]
    fn seq_regression_is_rejected() {
        let health = AccountStreamHealth::new(1);
        parse_account_event(r#"{"seq":9,"channel":"trade","data":{}}"#, &health).unwrap();
        assert!(parse_account_event(r#"{"seq":8,"channel":"trade","data":{}}"#, &health,).is_err());
    }

    #[tokio::test]
    async fn connect_waits_for_auth_and_delivers_order() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(socket).await.unwrap();
            let auth = websocket
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
            assert_eq!(auth["auth"]["token"], "jwt");
            websocket
                .send(Message::Text(
                    r#"{"seq":1,"channel":"auth","data":{"code":200,"msg":"success"}}"#.into(),
                ))
                .await
                .unwrap();
            websocket
                .send(Message::Text(
                    r#"{"seq":2,"channel":"order","data":{"id":7,"cl_ord_id":"sxmk-run-q1b0","symbol":"XAG-USD","side":"buy","qty":"0.200","fill_qty":"0.200","fill_avg_price":"58.87","price":"58.87","status":"filled","updated_at":"now"}}"#.into(),
                ))
                .await
                .unwrap();
        });

        let stream = AccountStream::with_url_and_token(url, "jwt", 4);
        let (mut events, health, handle) = stream
            .connect(&[AccountChannel::Order, AccountChannel::Position])
            .await
            .unwrap();
        assert_eq!(
            events.recv().await,
            Some(AccountEvent::Connected { epoch: 4 })
        );
        assert!(
            matches!(events.recv().await, Some(AccountEvent::Order(order)) if order.order_id == 7)
        );
        server.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while health.is_healthy() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        handle.await.unwrap();
    }
}
