//! Correlated asynchronous responses for HTTP order requests.

use crate::auth::Credentials;
use crate::error::{Error, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_ORDER_RESPONSE_URL: &str = "wss://perps.standx.com/ws-api/v1";

/// Asynchronous acceptance or rejection for an order request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderResponse {
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

impl OrderResponse {
    pub fn accepted(&self) -> bool {
        self.code == 0
    }
}

/// WebSocket stream paired with the `x-session-id` used by HTTP order calls.
pub struct OrderResponseStream {
    url: String,
    token: String,
    session_id: String,
}

/// Shared liveness flag for an authenticated order-response connection.
pub type OrderResponseHealth = Arc<AtomicBool>;

impl OrderResponseStream {
    /// Construct a production stream from the currently-loaded credentials.
    pub fn new(session_id: impl Into<String>) -> Result<Self> {
        let credentials = Credentials::load()?;
        if credentials.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        Ok(Self {
            url: DEFAULT_ORDER_RESPONSE_URL.to_string(),
            token: credentials.token,
            session_id: session_id.into(),
        })
    }

    #[cfg(test)]
    fn with_url_and_token(
        url: impl Into<String>,
        token: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            session_id: session_id.into(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Connect, wait for a successful authentication acknowledgement, and
    /// return parsed asynchronous order responses plus a shared liveness flag.
    pub async fn connect(
        &self,
    ) -> Result<(
        mpsc::Receiver<OrderResponse>,
        OrderResponseHealth,
        tokio::task::JoinHandle<()>,
    )> {
        let (stream, _) = connect_async(&self.url).await?;
        let (mut write, mut read) = stream.split();
        let auth_request_id = uuid::Uuid::new_v4().to_string();
        let auth = auth_request(&self.session_id, &self.token, &auth_request_id);
        write.send(Message::Text(auth.to_string().into())).await?;

        let auth_response = loop {
            let message = tokio::time::timeout(Duration::from_secs(10), read.next())
                .await
                .map_err(|_| Error::WebSocket {
                    message: "timed out waiting for order-response authentication".to_string(),
                })?
                .ok_or_else(|| Error::WebSocket {
                    message: "order-response stream closed before authentication".to_string(),
                })??;
            match message {
                Message::Text(text) => {
                    let response = serde_json::from_str::<OrderResponse>(&text)?;
                    if response.request_id.as_deref() != Some(auth_request_id.as_str()) {
                        return Err(Error::WebSocket {
                            message: "received an unexpected response before authentication"
                                .to_string(),
                        });
                    }
                    break response;
                }
                Message::Ping(payload) => write.send(Message::Pong(payload)).await?,
                Message::Close(_) => {
                    return Err(Error::WebSocket {
                        message: "order-response stream closed before authentication".to_string(),
                    });
                }
                _ => {}
            }
        };
        if !auth_response.accepted() {
            return Err(Error::AuthRequired {
                message: format!(
                    "order-response authentication rejected: {}",
                    auth_response.message
                ),
                resolution: "Run 'standx auth login' and retry".to_string(),
            });
        }

        let (tx, rx) = mpsc::channel(256);
        let healthy = Arc::new(AtomicBool::new(true));
        let task_health = Arc::clone(&healthy);
        let handle = tokio::spawn(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        if let Ok(response) = serde_json::from_str::<OrderResponse>(&text) {
                            if tx.send(response).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if write.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            task_health.store(false, Ordering::Release);
        });

        Ok((rx, healthy, handle))
    }
}

fn auth_request(session_id: &str, token: &str, request_id: &str) -> serde_json::Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "method": "auth:login",
        "params": serde_json::json!({ "token": token }).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::accept_async;

    #[test]
    fn auth_request_uses_stable_session_and_unique_request() {
        let first = auth_request("maker-session", "jwt", "request-1");
        let second = auth_request("maker-session", "jwt", "request-2");

        assert_eq!(first["session_id"], "maker-session");
        assert_eq!(first["method"], "auth:login");
        assert_eq!(first["request_id"], "request-1");
        assert_ne!(first["request_id"], second["request_id"]);
        assert!(first["params"].as_str().unwrap().contains("jwt"));
    }

    #[test]
    fn parses_acceptance_and_rejection_responses() {
        let accepted: OrderResponse = serde_json::from_value(serde_json::json!({
            "code": 0,
            "message": "success",
            "request_id": "request-1"
        }))
        .unwrap();
        assert!(accepted.accepted());

        let rejected: OrderResponse = serde_json::from_value(serde_json::json!({
            "code": 400,
            "message": "alo order rejected",
            "request_id": "request-2"
        }))
        .unwrap();
        assert!(!rejected.accepted());
    }

    #[test]
    fn test_constructor_keeps_session_id() {
        let stream = OrderResponseStream::with_url_and_token(
            "ws://localhost.invalid",
            "jwt",
            "maker-session",
        );
        assert_eq!(stream.session_id(), "maker-session");
    }

    #[tokio::test]
    async fn authenticated_connection_becomes_unhealthy_after_server_close() {
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
            websocket
                .send(Message::Text(
                    serde_json::json!({
                        "code": 0,
                        "message": "authenticated",
                        "request_id": auth["request_id"],
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            // Drop the socket: the client must flip the liveness flag rather
            // than continuing to place orders on a dead response stream.
        });

        let stream = OrderResponseStream::with_url_and_token(url, "jwt", "maker-session");
        let (_responses, health, handle) = stream.connect().await.unwrap();
        assert!(health.load(Ordering::Acquire));
        server.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while health.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("connection close should mark the response stream unhealthy");
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn authentication_rejection_prevents_connection_start() {
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
            websocket
                .send(Message::Text(
                    serde_json::json!({
                        "code": 401,
                        "message": "invalid token",
                        "request_id": auth["request_id"],
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let stream = OrderResponseStream::with_url_and_token(url, "bad-jwt", "maker-session");
        let error = stream.connect().await.unwrap_err();
        assert!(matches!(error, Error::AuthRequired { .. }));
        server.await.unwrap();
    }
}
