//! Correlated asynchronous responses for HTTP order requests.

use crate::auth::Credentials;
use crate::error::{Error, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_ORDER_RESPONSE_URL: &str = "wss://perps.standx.com/ws-api/v1";
/// If no inbound frame arrives within this window the connection is treated as
/// stale. The server sends a WebSocket ping every ~10s, so any healthy
/// connection produces inbound frames well inside this deadline; a longer gap
/// means the socket is half-open (peer gone, no error or close frame) and the
/// maker must stop trusting the confirmation stream instead of placing live
/// orders whose acknowledgements can never arrive.
const ORDER_RESPONSE_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

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
    idle_timeout: Duration,
}

/// Shared liveness state for an authenticated order-response connection.
///
/// The failure reason is written before `healthy` flips to false, so callers
/// that observe an unhealthy stream can include the close code/reason or the
/// underlying WebSocket error in their fail-safe log.
#[derive(Debug, Clone)]
pub struct OrderResponseHealth {
    healthy: Arc<AtomicBool>,
    failure_reason: Arc<Mutex<Option<String>>>,
}

impl Default for OrderResponseHealth {
    fn default() -> Self {
        Self {
            healthy: Arc::new(AtomicBool::new(true)),
            failure_reason: Arc::new(Mutex::new(None)),
        }
    }
}

impl OrderResponseHealth {
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    pub fn failure_reason(&self) -> Option<String> {
        self.failure_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }

    /// Mark the response stream unusable. This is public so the supervised
    /// controlled-disconnect hook can exercise the same production fail-safe.
    pub fn mark_unhealthy(&self, reason: impl Into<String>) {
        if let Ok(mut failure_reason) = self.failure_reason.lock() {
            *failure_reason = Some(reason.into());
        }
        self.healthy.store(false, Ordering::Release);
    }
}

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
            idle_timeout: ORDER_RESPONSE_IDLE_TIMEOUT,
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
            idle_timeout: ORDER_RESPONSE_IDLE_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
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
        let health = OrderResponseHealth::default();
        let task_health = health.clone();
        let idle_timeout = self.idle_timeout;
        let handle = tokio::spawn(async move {
            // Read-side idle deadline, reset on every inbound frame. This is the
            // only defence against a half-open socket: the peer stops sending
            // (including its ~10s server ping) but the connection never errors
            // or delivers a close frame, so `read.next()` would otherwise block
            // forever with the stream still reported healthy.
            let idle = tokio::time::sleep(idle_timeout);
            tokio::pin!(idle);
            loop {
                let message = tokio::select! {
                    _ = &mut idle => {
                        task_health.mark_unhealthy(format!(
                            "order-response stream idle for {}s (no ping/pong/data; connection likely half-open)",
                            idle_timeout.as_secs()
                        ));
                        return;
                    }
                    message = read.next() => message,
                };
                // Any inbound frame proves the peer is alive; extend the deadline.
                idle.as_mut()
                    .reset(tokio::time::Instant::now() + idle_timeout);
                let Some(message) = message else {
                    task_health.mark_unhealthy(
                        "order-response WebSocket ended without a close frame or reported error",
                    );
                    return;
                };
                match message {
                    Ok(Message::Text(text)) => {
                        if let Ok(response) = serde_json::from_str::<OrderResponse>(&text) {
                            if tx.send(response).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if let Err(error) = write.send(Message::Pong(payload)).await {
                            task_health.mark_unhealthy(format!(
                                "failed to send order-response pong: {error}"
                            ));
                            return;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        let reason = frame.map_or_else(
                            || "order-response WebSocket closed without a close frame".to_string(),
                            |frame| {
                                format!(
                                    "order-response WebSocket closed: code={} reason={:?}",
                                    u16::from(frame.code),
                                    frame.reason
                                )
                            },
                        );
                        task_health.mark_unhealthy(reason);
                        return;
                    }
                    Err(error) => {
                        task_health
                            .mark_unhealthy(format!("order-response WebSocket error: {error}"));
                        return;
                    }
                    _ => {}
                }
            }
        });

        Ok((rx, health, handle))
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
    use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

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
        assert!(health.is_healthy());
        server.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while health.is_healthy() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("connection close should mark the response stream unhealthy");
        handle.await.unwrap();
        assert!(health
            .failure_reason()
            .is_some_and(|reason| reason.contains("order-response WebSocket")));
    }

    #[tokio::test]
    async fn idle_connection_is_marked_unhealthy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        // Server authenticates then stays connected but silent — no data, no
        // server ping, no close frame — simulating a half-open connection that
        // never errors. Only the client's idle deadline can detect this.
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
            // Hold the socket open and silent, absorbing any client frames so
            // the client can only detect death via the idle timeout.
            while let Some(Ok(_)) = websocket.next().await {}
        });

        // Idle timeout far below the 10s server-ping cadence so the deadline,
        // not a real close/error, is what trips health.
        let stream = OrderResponseStream::with_url_and_token(url, "jwt", "maker-session")
            .with_idle_timeout(Duration::from_millis(200));
        let (_responses, health, handle) = stream.connect().await.unwrap();
        assert!(health.is_healthy());
        tokio::time::timeout(Duration::from_secs(2), async {
            while health.is_healthy() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("idle timeout should mark the response stream unhealthy");
        handle.await.unwrap();
        let reason = health.failure_reason().unwrap();
        assert!(reason.contains("idle"), "{reason}");
        server.abort();
    }

    #[tokio::test]
    async fn authenticated_close_preserves_code_and_reason() {
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
            websocket
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Policy,
                    reason: "maintenance".into(),
                })))
                .await
                .unwrap();
        });

        let stream = OrderResponseStream::with_url_and_token(url, "jwt", "maker-session");
        let (_responses, health, handle) = stream.connect().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while health.is_healthy() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("close frame should mark the response stream unhealthy");
        handle.await.unwrap();
        server.await.unwrap();

        let reason = health.failure_reason().unwrap();
        assert!(reason.contains("code=1008"), "{reason}");
        assert!(reason.contains("maintenance"), "{reason}");
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
