//! Order API client methods

use crate::auth::{Credentials, StandXSigner};
use crate::client::StandXClient;
use crate::error::{Error, Result};
use crate::models::{Order, OrderSide, OrderType, TimeInForce};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;

/// Order request parameters
#[derive(Debug, Clone)]
pub struct CreateOrderParams {
    pub symbol: String,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: String,
    pub price: Option<String>,
    pub time_in_force: Option<TimeInForce>,
    pub reduce_only: bool,
    pub stop_price: Option<String>,
    pub sl_price: Option<String>,
    pub tp_price: Option<String>,
}

impl Default for CreateOrderParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: String::new(),
            price: None,
            time_in_force: None,
            reduce_only: false,
            stop_price: None,
            sl_price: None,
            tp_price: None,
        }
    }
}

/// Order API methods
impl StandXClient {
    /// Build authenticated headers with optional request signing
    pub(super) async fn build_auth_headers(&self, payload: Option<&str>) -> Result<HeaderMap> {
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

        // Add request signature if private key is available
        if !creds.private_key.is_empty() {
            if let Ok(signer) = StandXSigner::from_base58(&creds.private_key) {
                let payload_str = payload.unwrap_or("");
                let signature = signer.sign_request_now(payload_str);

                headers.insert(
                    "x-request-sign-version",
                    HeaderValue::from_str(&signature.version).unwrap(),
                );
                headers.insert(
                    "x-request-id",
                    HeaderValue::from_str(&signature.request_id).unwrap(),
                );
                headers.insert(
                    "x-request-timestamp",
                    HeaderValue::from_str(&signature.timestamp.to_string()).unwrap(),
                );
                headers.insert(
                    "x-request-signature",
                    HeaderValue::from_str(&signature.signature).unwrap(),
                );
            }
        }

        Ok(headers)
    }

    /// Create a new order
    pub async fn create_order(&self, params: CreateOrderParams) -> Result<Order> {
        let url = format!("{}/api/new_order", self.base_url);

        // Build request body
        let order_type = match params.order_type {
            OrderType::Market => "market",
            OrderType::Limit => "limit",
        };

        let side = match params.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let tif = params
            .time_in_force
            .map(|t| match t {
                TimeInForce::Gtc => "gtc",
                TimeInForce::Ioc => "ioc",
                TimeInForce::Fok => "fok",
            })
            .unwrap_or("gtc");

        let mut body = json!({
            "symbol": params.symbol,
            "side": side,
            "order_type": order_type,
            "qty": params.quantity,
            "time_in_force": tif,
            "reduce_only": params.reduce_only,
        });

        // Add optional fields
        if let Some(ref price) = params.price {
            body["price"] = json!(price);
        }
        if let Some(stop_price) = params.stop_price {
            body["stop_price"] = json!(stop_price);
        }
        if let Some(sl_price) = params.sl_price {
            body["sl_price"] = json!(sl_price);
        }
        if let Some(tp_price) = params.tp_price {
            body["tp_price"] = json!(tp_price);
        }

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
                endpoint: Some("/api/new_order".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        let result: serde_json::Value = response.json().await?;

        // Check for API error
        if let Some(code) = result.get("code").and_then(|c| c.as_i64()) {
            if code != 0 {
                let message = result
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Order rejected");
                return Err(Error::Api {
                    code: code as u16,
                    message: message.to_string(),
                    endpoint: Some("/api/new_order".to_string()),
                    retryable: false,
                });
            }
        }

        // Build order from response
        let now = chrono::Utc::now().to_rfc3339();
        let order = Order {
            id: result
                .get("request_id")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string(),
            symbol: params.symbol,
            side: params.side,
            order_type: params.order_type,
            qty: params.quantity,
            fill_qty: "0".to_string(),
            price: params.price.clone().unwrap_or_else(|| "0".to_string()),
            status: crate::models::OrderStatus::New,
            created_at: now.clone(),
            updated_at: now,
        };

        Ok(order)
    }

    /// Cancel an order by ID
    pub async fn cancel_order(&self, symbol: &str, order_id: &str) -> Result<()> {
        let url = format!("{}/api/cancel_order", self.base_url);

        let body = json!({
            "symbol": symbol,
            "order_id": order_id.parse::<i64>().unwrap_or(0),
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
                endpoint: Some("/api/cancel_order".to_string()),
                retryable: status.as_u16() >= 500,
            });
        }

        Ok(())
    }

    /// Cancel all orders for a symbol
    pub async fn cancel_all_orders(&self, symbol: &str) -> Result<()> {
        let url = format!("{}/api/cancel_orders", self.base_url);

        let body = json!({
            "symbol": symbol,
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
                endpoint: Some("/api/cancel_orders".to_string()),
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
    async fn test_create_order_success() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/api/new_order")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":0,"request_id":"12345"}"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let params = CreateOrderParams {
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: "0.1".to_string(),
            price: Some("65000".to_string()),
            ..Default::default()
        };

        // Should fail because no credentials
        let result = client.create_order(params).await;
        assert!(result.is_err());
    }
}
