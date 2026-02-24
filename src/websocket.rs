//! WebSocket client for StandX real-time data

use crate::auth::Credentials;
use crate::error::{Error, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_WS_URL: &str = "wss://perps.standx.com/ws-stream/v1";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY_BASE: Duration = Duration::from_secs(2);
const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(30);

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel", rename_all = "snake_case")]
pub enum WsMessage {
    Auth { data: AuthData },
    DepthBook { data: DepthBookData, seq: u64 },
    Price { data: PriceData },
    Trade { data: TradeData },
    Order { data: serde_json::Value },
    Position { data: serde_json::Value },
    Balance { data: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthData {
    pub code: i32,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthBookData {
    pub symbol: String,
    pub bids: Vec<[serde_json::Value; 2]>,
    pub asks: Vec<[serde_json::Value; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub symbol: String,
    pub mark_price: String,
    pub index_price: String,
    pub last_price: String,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub symbol: String,
    pub price: String,
    pub qty: String,
    pub time: String,
    pub is_buyer_taker: bool,
}

/// WebSocket client state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    Authenticated,
}

/// WebSocket client for StandX
#[allow(dead_code)]
pub struct StandXWebSocket {
    url: String,
    token: String,
    state: Arc<RwLock<WsState>>,
    subscriptions: Arc<RwLock<Vec<Subscription>>>,
    message_tx: mpsc::Sender<WsMessage>,
    message_rx: Arc<RwLock<mpsc::Receiver<WsMessage>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
}

#[derive(Debug, Clone)]
struct Subscription {
    channel: String,
    symbol: Option<String>,
}

impl StandXWebSocket {
    /// Create a new WebSocket client
    pub fn new() -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired);
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
        })
    }

    /// Create with custom WebSocket URL
    pub fn with_url(url: String) -> Result<Self> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired);
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
        })
    }

    /// Connect and start the WebSocket client
    pub async fn connect(&self) -> Result<mpsc::Receiver<WsMessage>> {
        let (tx, rx) = mpsc::channel(100);

        // Clone Arc pointers for the task
        let url = self.url.clone();
        let token = self.token.clone();
        let state = Arc::clone(&self.state);
        let subscriptions = Arc::clone(&self.subscriptions);
        let reconnect_attempts = Arc::clone(&self.reconnect_attempts);

        // Spawn connection task
        tokio::spawn(async move {
            Self::connection_task(url, token, state, subscriptions, reconnect_attempts, tx).await;
        });

        Ok(rx)
    }

    /// Subscribe to a channel
    pub async fn subscribe(&self, channel: &str, symbol: Option<&str>) {
        let mut subs = self.subscriptions.write().await;
        subs.push(Subscription {
            channel: channel.to_string(),
            symbol: symbol.map(|s| s.to_string()),
        });
    }

    /// Get current connection state
    pub async fn state(&self) -> WsState {
        *self.state.read().await
    }

    /// Connection management task
    async fn connection_task(
        url: String,
        token: String,
        state: Arc<RwLock<WsState>>,
        subscriptions: Arc<RwLock<Vec<Subscription>>>,
        reconnect_attempts: Arc<RwLock<u32>>,
        message_tx: mpsc::Sender<WsMessage>,
    ) {
        loop {
            // Update state to connecting
            {
                let mut s = state.write().await;
                *s = WsState::Connecting;
            }

            match Self::run_connection(&url, &token, &state, &subscriptions, &message_tx).await {
                Ok(()) => {
                    // Connection closed normally
                    tracing::info!("WebSocket connection closed");
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {}", e);
                }
            }

            // Increment reconnect attempts
            {
                let mut attempts = reconnect_attempts.write().await;
                *attempts += 1;
            }

            // Calculate backoff delay
            let delay = Self::calculate_backoff(&reconnect_attempts).await;
            tracing::info!("Reconnecting in {:?}...", delay);
            tokio::time::sleep(delay).await;
        }
    }

    /// Run a single WebSocket connection
    async fn run_connection(
        url: &str,
        token: &str,
        state: &Arc<RwLock<WsState>>,
        subscriptions: &Arc<RwLock<Vec<Subscription>>>,
        message_tx: &mpsc::Sender<WsMessage>,
    ) -> Result<()> {
        // Connect to WebSocket
        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| Error::Unknown(format!("WebSocket connect failed: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();

        // Update state to connected
        {
            let mut s = state.write().await;
            *s = WsState::Connected;
        }

        // Send authentication
        let auth_msg = json!({
            "auth": {
                "token": token
            }
        });
        write
            .send(Message::Text(auth_msg.to_string().into()))
            .await
            .map_err(|e| Error::Unknown(format!("Failed to send auth: {}", e)))?;

        // Start heartbeat
        let mut heartbeat = interval(HEARTBEAT_INTERVAL);
        let mut last_pong = Instant::now();

        loop {
            tokio::select! {
                // Handle incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            // Parse and handle message
                            if let Ok(message) = serde_json::from_str::<WsMessage>(&text) {
                                // Update state on auth success
                                if let WsMessage::Auth { data } = &message {
                                    if data.code == 200 || data.code == 0 {
                                        let mut s = state.write().await;
                                        *s = WsState::Authenticated;

                                        // Resubscribe to channels
                                        let subs = subscriptions.read().await;
                                        for sub in subs.iter() {
                                            let sub_msg = json!({
                                                "subscribe": {
                                                    "channel": sub.channel,
                                                    "symbol": sub.symbol
                                                }
                                            });
                                            let _ = write.send(Message::Text(sub_msg.to_string().into())).await;
                                        }
                                    }
                                }

                                // Forward message
                                let _ = message_tx.send(message).await;
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            last_pong = Instant::now();
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break;
                        }
                        Some(Err(e)) => {
                            return Err(Error::Unknown(format!("WebSocket error: {}", e)));
                        }
                        _ => {}
                    }
                }

                // Send heartbeat ping
                _ = heartbeat.tick() => {
                    // Check if we've received a pong recently
                    if last_pong.elapsed() > HEARTBEAT_INTERVAL * 2 {
                        return Err(Error::Unknown("Heartbeat timeout".to_string()));
                    }

                    write.send(Message::Ping(vec![].into())).await
                        .map_err(|e| Error::Unknown(format!("Failed to send ping: {}", e)))?;
                }
            }
        }

        // Update state to disconnected
        {
            let mut s = state.write().await;
            *s = WsState::Disconnected;
        }

        Ok(())
    }

    /// Calculate reconnect backoff delay
    async fn calculate_backoff(reconnect_attempts: &Arc<RwLock<u32>>) -> Duration {
        let attempts = *reconnect_attempts.read().await;
        let delay = RECONNECT_DELAY_BASE * 2_u32.pow(attempts.min(4));
        delay.min(RECONNECT_DELAY_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_state() {
        assert_ne!(WsState::Disconnected, WsState::Connected);
        assert_ne!(WsState::Connecting, WsState::Authenticated);
    }
}
