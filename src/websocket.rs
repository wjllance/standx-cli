//! WebSocket client for real-time data

use crate::auth::Credentials;
use crate::error::{Error, Result};
use crate::models::*;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_WS_URL: &str = "wss://perps.standx.com/ws-stream/v1";
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// WebSocket client state
#[derive(Debug, Clone, PartialEq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// WebSocket message wrapper
#[derive(Debug, Clone)]
pub enum WsMessage {
    Connected,
    Disconnected,
    Price(PriceData),
    Depth(OrderBook),
    Trade(Trade),
    Position(Position),
    Balance(Balance),
    Order(Order),
    AccountUpdate(String),
    Error(String),
    Heartbeat,
}

/// Subscription target for a channel (and optional symbol)
#[derive(Debug, Clone)]
pub struct Subscription {
    pub channel: String,
    pub symbol: Option<String>,
}

impl Subscription {
    pub fn new(channel: &str, symbol: Option<&str>) -> Self {
        Self {
            channel: channel.to_string(),
            symbol: symbol.map(String::from),
        }
    }
}

/// StandX WebSocket client
pub struct StandXWebSocket {
    url: String,
    token: String,
    state: Arc<RwLock<WsState>>,
    subscriptions: Arc<RwLock<Vec<Subscription>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
}

impl StandXWebSocket {
    /// Create a new WebSocket client
    pub fn new() -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        Ok(Self {
            url: DEFAULT_WS_URL.to_string(),
            token: creds.token,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            reconnect_attempts: Arc::new(RwLock::new(0)),
        })
    }

    /// Create with custom WebSocket URL
    pub fn with_url(url: String) -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        Ok(Self {
            url,
            token: creds.token,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            reconnect_attempts: Arc::new(RwLock::new(0)),
        })
    }

    /// Connect and start the WebSocket client. Returns the receiver for stream messages.
    pub async fn connect(&self) -> Result<mpsc::Receiver<WsMessage>> {
        let (message_tx, message_rx) = mpsc::channel(100);

        let url = self.url.clone();
        let token = self.token.clone();
        let state = self.state.clone();
        let subs: Vec<Subscription> = self.subscriptions.read().await.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();

        tokio::spawn(async move {
            loop {
                *state.write().await = WsState::Connecting;

                match connect_and_run(&url, &token, &subs, &message_tx).await {
                    Ok(_) => {
                        *reconnect_attempts.write().await = 0;
                    }
                    Err(e) => {
                        let attempts = *reconnect_attempts.read().await;
                        if attempts >= 5 {
                            let _ = message_tx
                                .send(WsMessage::Error(format!(
                                    "Max reconnection attempts reached: {}",
                                    e
                                )))
                                .await;
                            break;
                        }

                        *reconnect_attempts.write().await = attempts + 1;
                        *state.write().await = WsState::Reconnecting;

                        tokio::time::sleep(RECONNECT_DELAY).await;
                    }
                }
            }
        });

        Ok(message_rx)
    }

    /// Subscribe to a channel (and optional symbol for market channels)
    pub async fn subscribe(&self, channel: &str, symbol: Option<&str>) -> Result<()> {
        let mut subs = self.subscriptions.write().await;
        subs.push(Subscription::new(channel, symbol));
        Ok(())
    }

    /// Get current state
    pub async fn state(&self) -> WsState {
        self.state.read().await.clone()
    }
}

async fn connect_and_run(
    url: &str,
    token: &str,
    subscriptions: &[Subscription],
    message_tx: &mpsc::Sender<WsMessage>,
) -> Result<()> {
    let (ws_stream, _) = connect_async(url)
        .await
        .map_err(|e| Error::Unknown(format!("WebSocket connect failed: {}", e)))?;

    let (mut write, mut read) = ws_stream.split();

    // Auth: API expects { "auth": { "token": "...", "streams": [...] } }
    let auth_msg = serde_json::json!({
        "auth": {
            "token": token
        }
    });
    write
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .map_err(|e| Error::Unknown(format!("Failed to send auth: {}", e)))?;

    let _ = message_tx.send(WsMessage::Connected).await;

    // Subscribe to each channel
    for sub in subscriptions {
        let body: serde_json::Value = if let Some(ref sym) = sub.symbol {
            serde_json::json!({ "subscribe": { "channel": sub.channel, "symbol": sym } })
        } else {
            serde_json::json!({ "subscribe": { "channel": sub.channel } })
        };
        write
            .send(Message::Text(body.to_string().into()))
            .await
            .map_err(|e| Error::Unknown(format!("Failed to send subscribe: {}", e)))?;
    }

    // Spawn heartbeat task (respond to server ping with pong)
    let heartbeat_tx = message_tx.clone();
    let heartbeat_write = Arc::new(RwLock::new(write));
    let heartbeat_write_clone = heartbeat_write.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            interval.tick().await;
            let mut writer = heartbeat_write_clone.write().await;
            if let Err(e) = writer.send(Message::Ping(vec![].into())).await {
                let _ = heartbeat_tx
                    .send(WsMessage::Error(format!("Heartbeat failed: {}", e)))
                    .await;
                break;
            }
        }
    });

    // Main message loop: API responses are { "seq", "channel", "symbol"?, "data": { ... } }
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(root) = serde_json::from_str::<serde_json::Value>(&text) {
                    let channel = root.get("channel").and_then(|c| c.as_str());
                    let data = root.get("data").cloned();

                    if let (Some(ch), Some(d)) = (channel, data) {
                        match ch {
                            "price" => {
                                if let Ok(price) = serde_json::from_value::<PriceData>(d) {
                                    let _ = message_tx.send(WsMessage::Price(price)).await;
                                }
                            }
                            "depth_book" => {
                                if let Ok(depth) = serde_json::from_value::<OrderBook>(d) {
                                    let _ = message_tx.send(WsMessage::Depth(depth)).await;
                                }
                            }
                            "public_trade" => {
                                if let Ok(trade) = serde_json::from_value::<Trade>(d) {
                                    let _ = message_tx.send(WsMessage::Trade(trade)).await;
                                }
                            }
                            "order" => {
                                if let Ok(order) = serde_json::from_value::<Order>(d) {
                                    let _ = message_tx.send(WsMessage::Order(order)).await;
                                }
                            }
                            "position" => {
                                if let Ok(pos) = serde_json::from_value::<Position>(d) {
                                    let _ = message_tx.send(WsMessage::Position(pos)).await;
                                }
                            }
                            "balance" => {
                                if let Ok(bal) = serde_json::from_value::<Balance>(d) {
                                    let _ = message_tx.send(WsMessage::Balance(bal)).await;
                                }
                            }
                            "auth" => {
                                // Auth response e.g. { "code": 200, "msg": "success" }
                                if let Some(code) = d.get("code").and_then(|c| c.as_i64()) {
                                    if code != 200 {
                                        let msg = d.get("msg").and_then(|m| m.as_str()).unwrap_or("auth failed");
                                        let _ = message_tx.send(WsMessage::Error(msg.to_string())).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let mut writer = heartbeat_write.write().await;
                if let Err(e) = writer.send(Message::Pong(data)).await {
                    return Err(Error::Unknown(format!("Failed to send pong: {}", e)));
                }
            }
            Ok(Message::Pong(_)) => {
                let _ = message_tx.send(WsMessage::Heartbeat).await;
            }
            Ok(Message::Close(_)) => {
                let _ = message_tx.send(WsMessage::Disconnected).await;
                break;
            }
            Err(e) => {
                return Err(Error::Unknown(format!("WebSocket error: {}", e)));
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_state() {
        assert_ne!(WsState::Connected, WsState::Disconnected);
    }

    #[test]
    fn test_subscription_new() {
        let s = Subscription::new("price", Some("BTC-USD"));
        assert_eq!(s.channel, "price");
        assert_eq!(s.symbol.as_deref(), Some("BTC-USD"));
        let s2 = Subscription::new("order", None);
        assert_eq!(s2.symbol, None);
    }
}
