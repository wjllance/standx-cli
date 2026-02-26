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
    token: Option<String>,
    state: Arc<RwLock<WsState>>,
    subscriptions: Arc<RwLock<Vec<String>>>,
    #[allow(dead_code)]
    message_tx: mpsc::Sender<WsMessage>,
    #[allow(dead_code)]
    message_rx: Arc<RwLock<mpsc::Receiver<WsMessage>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
    #[allow(dead_code)]
    channel: String,
    #[allow(dead_code)]
    symbol: Option<String>,
    verbose: bool,
}

impl StandXWebSocket {
    /// Create a new WebSocket client (requires auth for user channels)
    pub fn new() -> Result<Self> {
        Self::new_with_verbose(false)
    }

    /// Create a new WebSocket client with verbose mode
    pub fn new_with_verbose(verbose: bool) -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        let (message_tx, message_rx) = mpsc::channel(100);

        Ok(Self {
            url: DEFAULT_WS_URL.to_string(),
            token: Some(creds.token),
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            channel: String::new(),
            symbol: None,
            verbose,
        })
    }

    /// Create without authentication (for public channels only)
    pub fn without_auth() -> Result<Self> {
        Self::without_auth_with_verbose(false)
    }

    /// Create without authentication with verbose mode
    pub fn without_auth_with_verbose(verbose: bool) -> Result<Self> {
        let (message_tx, message_rx) = mpsc::channel(100);

        Ok(Self {
            url: DEFAULT_WS_URL.to_string(),
            token: None,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            channel: String::new(),
            symbol: None,
            verbose,
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

        let (message_tx, message_rx) = mpsc::channel(100);

        Ok(Self {
            url,
            token: Some(creds.token),
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            channel: String::new(),
            symbol: None,
            verbose: false,
        })
    }

    /// Connect and start the WebSocket client
    pub async fn connect(&self) -> Result<mpsc::Receiver<WsMessage>> {
        let (tx, rx) = mpsc::channel(100);

        let url = self.url.clone();
        let token = self.token.clone();
        let state = self.state.clone();
        let subscriptions = self.subscriptions.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();
        let verbose = self.verbose;

        tokio::spawn(async move {
            loop {
                *state.write().await = WsState::Connecting;

                match connect_and_run(&url, token.as_deref(), &subscriptions, &tx, verbose).await {
                    Ok(_) => {
                        *reconnect_attempts.write().await = 0;
                    }
                    Err(e) => {
                        let attempts = *reconnect_attempts.read().await;
                        if attempts >= 5 {
                            let _ = tx
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
    token: Option<&str>,
    subscriptions: &Arc<RwLock<Vec<String>>>,
    message_tx: &mpsc::Sender<WsMessage>,
    verbose: bool,
) -> Result<()> {
    let ws_url = url.to_string();
    if verbose {
        eprintln!("[WebSocket Debug] Connecting to: {}", ws_url);
    }

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| Error::Unknown(format!("WebSocket connect failed: {}", e)))?;
    if verbose {
        eprintln!("[WebSocket Debug] Connected successfully");
    }

    let (mut write, mut read) = ws_stream.split();

    // Get subscriptions early for auth message
    let subs = subscriptions.read().await;

    // Send authentication only if token is provided
    if let Some(t) = token {
        // Build streams array from subscriptions
        let streams: Vec<serde_json::Value> = subs
            .iter()
            .map(|topic| {
                let parts: Vec<&str> = topic.split(':').collect();
                let channel = parts[0];
                let symbol = if parts.len() > 1 { parts[1] } else { "" };
                if symbol.is_empty() {
                    serde_json::json!({ "channel": channel })
                } else {
                    serde_json::json!({ "channel": channel, "symbol": symbol })
                }
            })
            .collect();

        // Format token with Bearer prefix if not already present
        let token_with_bearer = if t.starts_with("Bearer ") {
            t.to_string()
        } else {
            format!("Bearer {}", t)
        };

        let auth_msg = serde_json::json!({
            "auth": {
                "token": token_with_bearer,
                "streams": streams
            }
        });
        if verbose {
            eprintln!("[WebSocket Debug] Sending auth: {}", auth_msg);
        }
        write
            .send(Message::Text(auth_msg.to_string().into()))
            .await
            .map_err(|e| Error::Unknown(format!("Failed to send auth: {}", e)))?;
        if verbose {
            eprintln!("[WebSocket Debug] Auth sent");
        }
    } else if verbose {
        eprintln!("[WebSocket Debug] Skipping auth (public channel)");
    }

    // Send subscription messages for all registered subscriptions
    if verbose {
        eprintln!("[WebSocket Debug] Subscribing to {} topics", subs.len());
    }

    // Wait a bit for server to be ready
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    for topic in subs.iter() {
        // Parse topic to get channel and symbol (format: "price:BTC-USD")
        let parts: Vec<&str> = topic.split(':').collect();
        let (channel, symbol) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (topic.as_str(), "")
        };

        let sub_msg = serde_json::json!({
            "subscribe": {
                "channel": channel,
                "symbol": symbol
            }
        });
        if verbose {
            eprintln!("[WebSocket Debug] Sending subscribe: {}", sub_msg);
        }
        if let Err(e) = write.send(Message::Text(sub_msg.to_string().into())).await {
            let _ = message_tx
                .send(WsMessage::Error(format!(
                    "Failed to subscribe to {}: {}",
                    topic, e
                )))
                .await;
        }
    }

    let _ = message_tx.send(WsMessage::Connected).await;
    if verbose {
        eprintln!("[WebSocket Debug] Entering message loop");
    }

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
                let _ = heartbeat_tx
                    .send(WsMessage::Error(format!("Heartbeat failed: {}", e)))
                    .await;
                break;
            }
        }
    });

    // Main message loop
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Debug: print received message
                if verbose {
                    eprintln!("[WebSocket Debug] Received: {}", text);
                }

                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Check for error response
                    if let Some(code) = data.get("code").and_then(|c| c.as_i64()) {
                        if code != 0 {
                            let message = data
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error");
                            if verbose {
                                eprintln!("[WebSocket Debug] Server error: {}", message);
                            }
                            continue;
                        }
                    }

                    // Parse message based on channel field
                    if let Some(channel) = data.get("channel").and_then(|c| c.as_str()) {
                        if verbose {
                            eprintln!("[WebSocket Debug] Message channel: {}", channel);
                        }
                        if let Some(data_obj) = data.get("data") {
                            match channel {
                                "price" => {
                                    if let Ok(price) =
                                        serde_json::from_value::<PriceData>(data_obj.clone())
                                    {
                                        let _ = message_tx.send(WsMessage::Price(price)).await;
                                    }
                                }
                                "depth_book" => {
                                    if let Ok(depth) =
                                        serde_json::from_value::<OrderBook>(data_obj.clone())
                                    {
                                        let _ = message_tx.send(WsMessage::Depth(depth)).await;
                                    }
                                }
                                "public_trade" => {
                                    if let Ok(trade) =
                                        serde_json::from_value::<Trade>(data_obj.clone())
                                    {
                                        let _ = message_tx.send(WsMessage::Trade(trade)).await;
                                    }
                                }
                                "order" | "position" | "balance" | "trade" => {
                                    if verbose {
                                        eprintln!(
                                            "[WebSocket Debug] User channel received: {}",
                                            channel
                                        );
                                    }
                                    // TODO: Parse user-specific messages
                                }
                                _ => {
                                    if verbose {
                                        eprintln!("[WebSocket Debug] Unknown channel: {}", channel);
                                    }
                                }
                            }
                        }
                    } else if verbose {
                        eprintln!("[WebSocket Debug] No channel field in message");
                    }
                } else if verbose {
                    eprintln!("[WebSocket Debug] Failed to parse JSON: {}", text);
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
            Ok(Message::Close(frame)) => {
                if verbose {
                    eprintln!("[WebSocket Debug] Connection closed: {:?}", frame);
                }
                let _ = message_tx.send(WsMessage::Disconnected).await;
                break;
            }
            Ok(Message::Frame(_)) => {
                // Frame messages are handled internally by tungstenite
            }
            Ok(Message::Binary(data)) => {
                if verbose {
                    eprintln!(
                        "[WebSocket Debug] Received binary data: {} bytes",
                        data.len()
                    );
                }
            }
            Err(e) => {
                if verbose {
                    eprintln!("[WebSocket Debug] WebSocket error: {}", e);
                }
                return Err(Error::Unknown(format!("WebSocket error: {}", e)));
            }
        }
    }

    if verbose {
        eprintln!("[WebSocket Debug] Message loop ended");
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
