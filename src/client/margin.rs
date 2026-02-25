//! Margin management API client methods

use crate::auth::Credentials;
use crate::client::StandXClient;
use crate::error::{Error, Result};
use crate::models::PositionConfig;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;

impl StandXClient {
    /// Build auth-only headers (JWT, no signature) for GET requests
    fn margin_auth_headers(&self) -> Result<HeaderMap> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {}", creds.token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).map_err(|e| Error::Api {
                code: 500,
                message: e.to_string(),
                endpoint: None,
                retryable: false,
            })?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    /// Transfer margin to/from an isolated position
    ///
    /// `amount_in` is a decimal string. Positive = deposit into position,
    /// negative = withdraw from position (if the exchange supports it).
    pub async fn transfer_margin(&self, symbol: &str, amount_in: &str) -> Result<()> {
        let url = format!("{}/api/transfer_margin", self.base_url);

        let body = json!({
            "symbol": symbol,
            "amount_in": amount_in,
        });

        let body_str = body.to_string();
        let headers = self.build_auth_headers(Some(&body_str)).await?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/transfer_margin".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let result: serde_json::Value = response.json().await?;
        if let Some(code) = result.get("code").and_then(|c| c.as_i64()) {
            if code != 0 {
                let message = result
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Transfer failed");
                return Err(Error::Api {
                    code: code as u16,
                    message: message.to_string(),
                    endpoint: Some("/api/transfer_margin".to_string()),
                    retryable: false,
                });
            }
        }

        Ok(())
    }

    /// Change margin mode for a symbol (cross or isolated)
    pub async fn change_margin_mode(&self, symbol: &str, margin_mode: &str) -> Result<()> {
        let url = format!("{}/api/change_margin_mode", self.base_url);

        let body = json!({
            "symbol": symbol,
            "margin_mode": margin_mode,
        });

        let body_str = body.to_string();
        let headers = self.build_auth_headers(Some(&body_str)).await?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/change_margin_mode".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let result: serde_json::Value = response.json().await?;
        if let Some(code) = result.get("code").and_then(|c| c.as_i64()) {
            if code != 0 {
                let message = result
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Change margin mode failed");
                return Err(Error::Api {
                    code: code as u16,
                    message: message.to_string(),
                    endpoint: Some("/api/change_margin_mode".to_string()),
                    retryable: false,
                });
            }
        }

        Ok(())
    }
}
