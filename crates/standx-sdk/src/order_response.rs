//! Correlated asynchronous responses for HTTP order requests.

use crate::auth::Credentials;
use crate::error::{Error, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
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

    /// Connect, authenticate, and return parsed asynchronous order responses.
    pub async fn connect(
        &self,
    ) -> Result<(mpsc::Receiver<OrderResponse>, tokio::task::JoinHandle<()>)> {
        let (stream, _) = connect_async(&self.url).await?;
        let (mut write, mut read) = stream.split();
        let auth = auth_request(&self.session_id, &self.token);
        write.send(Message::Text(auth.to_string().into())).await?;

        let (tx, rx) = mpsc::channel(256);
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
        });

        Ok((rx, handle))
    }
}

fn auth_request(session_id: &str, token: &str) -> serde_json::Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": uuid::Uuid::new_v4().to_string(),
        "method": "auth:login",
        "params": serde_json::json!({ "token": token }).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_request_uses_stable_session_and_unique_request() {
        let first = auth_request("maker-session", "jwt");
        let second = auth_request("maker-session", "jwt");

        assert_eq!(first["session_id"], "maker-session");
        assert_eq!(first["method"], "auth:login");
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
}
