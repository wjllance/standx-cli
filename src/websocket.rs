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

/// StandX WebSocket client
pub struct StandXWebSocket {
    url: String,
    token: String,
    state: Arc<RwLock<WsState>>,
    subscriptions: Arc<RwLock<Vec<String>>>,
    message_tx: mpsc::Sender<WsMessage>,
    message_rx: Arc<RwLock<mpsc::Receiver<WsMessage>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
    channel: String,
    symbol: Option<String>,
}

impl StandXWebSocket {
    /// Create a new WebSocket client
    pub fn new() -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable".to_string(),
            });
        }

        let (message_tx, message_rx) = mpsc::channel(100);

        Ok(Self {
            url: DEFAULT_WS_URL.to_string(),
            token: creds.token,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            channel: String::new(),
            symbol: None,
        })
    }

    /// Create with custom WebSocket URL
    pub fn with_url(url: String) -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable".to_string(),
            });
        }

        let (message_tx, message_rx) = mpsc::channel(100);

        Ok(Self {
            url,
            token: creds.token,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            channel: String::new(),
            symbol: None,
        })
    }

    /// Connect and start the WebSocket client
    pub async fn connect(&self) -> Result<mpsc::Receiver<WsMessage>> {
        let (tx, rx) = mpsc::channel(100);

        let url = self.url.clone();
        let token = self.token.clone();
        let state = self.state.clone();
        let subscriptions = self.subscriptions.clone();
        let message_tx = self.message_tx.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();

        tokio::spawn(async move {
            loop {
                *state.write().await = WsState::Connecting;

                match connect_and_run(&url, &token, &subscriptions, &message_tx).await {
                    Ok(_) => {
                        *reconnect_attempts.write().await = 0;
                    }
                    Err(e) => {
                        let attempts = *reconnect_attempts.read().await;
                        if attempts >= 5 {
                            let _ = message_tx.send(WsMessage::Error(format!(
                                "Max reconnection attempts reached: {}",
                                e
                            ))).await;
                            break;
                        }

                        *reconnect_attempts.write().await = attempts + 1;
                        *state.write().await = WsState::Reconnecting;

                        tokio::time::sleep(RECONNECT_DELAY).await;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Subscribe to a channel
    pub async fn subscribe(&self, channel: &str, symbol: Option<&str>) -> Result<()> {
        let mut subs = self.subscriptions.write().await;
        let topic = if let Some(sym) = symbol {
            format!("{}:{}", channel, sym)
        } else {
            channel.to_string()
        };
        subs.push(topic);
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
    _subscriptions: &Arc<RwLock<Vec<String>>>,
    message_tx: &mpsc::Sender<WsMessage>,
) -> Result<()> {
    let ws_url = format!("{}?token={}", url, token);

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| Error::Unknown { 
            0: format!("WebSocket connect failed: {}", e) 
        })?;

    let (mut write, mut read) = ws_stream.split();

    // Send authentication
    let auth_msg = serde_json::json!({
        "op": "auth",
        "token": token
    });
    write
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .map_err(|e| Error::Unknown { 
            0: format!("Failed to send auth: {}", e) 
        })?;

    let _ = message_tx.send(WsMessage::Connected).await;

    // Spawn heartbeat task
    let heartbeat_tx = message_tx.clone();
    let heartbeat_write = Arc::new(RwLock::new(write));
    let heartbeat_write_clone = heartbeat_write.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            interval.tick().await;
            let mut writer = heartbeat_write_clone.write().await;
            if let Err(e) = writer.send(Message::Ping(vec![].into())).await {
                let _ = heartbeat_tx.send(WsMessage::Error(format!("Heartbeat failed: {}", e))).await;
                break;
            }
        }
    });

    // Main message loop
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Parse message based on type
                    if let Some(msg_type) = data.get("type").and_then(|t| t.as_str()) {
                        match msg_type {
                            "price" => {
                                if let Ok(price) = serde_json::from_value::<PriceData>(data) {
                                    let _ = message_tx.send(WsMessage::Price(price)).await;
                                }
                            }
                            "depth" => {
                                if let Ok(depth) = serde_json::from_value::<OrderBook>(data) {
                                    let _ = message_tx.send(WsMessage::Depth(depth)).await;
                                }
                            }
                            "trade" => {
                                if let Ok(trade) = serde_json::from_value::<Trade>(data) {
                                    let _ = message_tx.send(WsMessage::Trade(trade)).await;
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
                    return Err(Error::Unknown { 
                        0: format!("Failed to send pong: {}", e) 
                    });
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
                return Err(Error::Unknown { 
                    0: format!("WebSocket error: {}", e) 
                });
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
}
