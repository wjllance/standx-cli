//! Account API client methods

use crate::auth::{Credentials, StandXSigner};
use crate::client::StandXClient;
use crate::error::{Error, Result};
use crate::models::{Balance, Order, Position};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

/// API response wrapper for list endpoints
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiListResponse<T> {
    code: i32,
    message: String,
    #[serde(rename = "page_size")]
    _page_size: Option<i32>,
    result: Vec<T>,
}

/// Account-related API methods
impl StandXClient {
    /// Load credentials and create authenticated headers
    fn auth_headers(&self) -> Result<HeaderMap> {
        let creds = Credentials::load()?;

        if creds.is_expired() {
            return Err(Error::AuthRequired {
                message: "Token expired".to_string(),
                resolution: "Run 'standx auth login' or set STANDX_JWT environment variable"
                    .to_string(),
            });
        }

        let mut headers = HeaderMap::new();

        // Authorization header with JWT
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

    /// Sign request with Ed25519
    #[allow(dead_code)]
    fn sign_request(&self, payload: &str) -> Result<(StandXSigner, crate::auth::RequestSignature)> {
        let creds = Credentials::load()?;
        let signer = StandXSigner::from_base58(&creds.private_key).map_err(|_| {
            Error::InvalidCredentials {
                message: "Invalid private key format".to_string(),
            }
        })?;

        let signature = signer.sign_request_now(payload);
        Ok((signer, signature))
    }

    /// Get account balances
    pub async fn get_balance(&self) -> Result<Balance> {
        let url = format!("{}/api/query_balance", self.base_url);
        let headers = self.auth_headers()?;

        let response = self.client.get(&url).headers(headers).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_balance".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let data = response.json::<Balance>().await?;
        Ok(data)
    }

    /// Get positions
    pub async fn get_positions(&self, symbol: Option<&str>) -> Result<Vec<Position>> {
        let url = format!("{}/api/query_positions", self.base_url);
        let headers = self.auth_headers()?;

        let mut query = vec![];
        if let Some(s) = symbol {
            query.push(("symbol", s));
        }

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_positions".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let data = response.json::<Vec<Position>>().await?;
        Ok(data)
    }

    /// Get open orders
    pub async fn get_open_orders(&self, symbol: Option<&str>) -> Result<Vec<Order>> {
        let url = format!("{}/api/query_open_orders", self.base_url);
        let headers = self.auth_headers()?;

        let mut query = vec![];
        if let Some(s) = symbol {
            query.push(("symbol", s));
        }

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_open_orders".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let wrapper = response.json::<ApiListResponse<Order>>().await?;
        Ok(wrapper.result)
    }

    /// Get order history
    pub async fn get_order_history(
        &self,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Order>> {
        let url = format!("{}/api/query_orders", self.base_url);
        let headers = self.auth_headers()?;

        let mut query: Vec<(&str, String)> = vec![];
        // status=filled means filled orders (history)
        query.push(("status", "filled".to_string()));
        if let Some(s) = symbol {
            query.push(("symbol", s.to_string()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_orders".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let wrapper = response.json::<ApiListResponse<Order>>().await?;
        Ok(wrapper.result)
    }

    /// Get user trade history
    pub async fn get_user_trades(
        &self,
        symbol: &str,
        from: i64,
        to: i64,
        limit: Option<u32>,
    ) -> Result<Vec<crate::models::Trade>> {
        let url = format!("{}/api/query_trades", self.base_url);
        let headers = self.auth_headers()?;

        let mut query: Vec<(&str, String)> = vec![
            ("symbol", symbol.to_string()),
            ("from", from.to_string()),
            ("to", to.to_string()),
        ];
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_trades".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let wrapper = response
            .json::<ApiListResponse<crate::models::Trade>>()
            .await?;
        Ok(wrapper.result)
    }

    /// Get position config (includes leverage)
    pub async fn get_position_config(&self, symbol: &str) -> Result<crate::models::PositionConfig> {
        let url = format!("{}/api/query_position_config", self.base_url);
        let headers = self.auth_headers()?;

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&[("symbol", symbol)])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
                endpoint: Some("/api/query_position_config".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let data = response.json::<crate::models::PositionConfig>().await?;
        Ok(data)
    }

    /// Change leverage for a symbol
    pub async fn change_leverage(&self, symbol: &str, leverage: u32) -> Result<()> {
        let url = format!("{}/api/change_leverage", self.base_url);

        let body = serde_json::json!({
            "symbol": symbol,
            "leverage": leverage,
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
                endpoint: Some("/api/change_leverage".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        Ok(())
    }

    /// Change margin mode for a symbol
    pub async fn change_margin_mode(&self, symbol: &str, mode: &str) -> Result<()> {
        let url = format!("{}/api/change_margin_mode", self.base_url);

        let body = serde_json::json!({
            "symbol": symbol,
            "margin_mode": mode,
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

        Ok(())
    }

    /// Transfer margin for a symbol
    pub async fn transfer_margin(
        &self,
        symbol: &str,
        amount: &str,
        direction: &str,
    ) -> Result<()> {
        let url = format!("{}/api/transfer_margin", self.base_url);

        // Convert amount to negative for withdraw
        let amount_val: f64 = amount.parse()
            .map_err(|_| Error::Api {
                code: 400,
                message: format!("Invalid amount: {}", amount),
                endpoint: None,
                retryable: false,
            })?;
        
        let final_amount = if direction == "withdraw" {
            -amount_val.abs()
        } else {
            amount_val.abs()
        };

        let body = serde_json::json!({
            "symbol": symbol,
            "amount_in": final_amount.to_string(),
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_get_balance_unauthorized() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/api/query_balance")
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        // Should fail because no credentials
        let result = client.get_balance().await;
        assert!(result.is_err());
    }
}
