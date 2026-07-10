//! Order API client methods

use crate::client::StandXClient;
use crate::error::{Error, Result};
use crate::models::{Order, OrderSide, OrderType, TimeInForce};
use serde_json::json;

/// Order request parameters
#[derive(Debug, Clone)]
pub struct CreateOrderParams {
    pub symbol: String,
    /// Client-generated idempotency/correlation ID.
    pub cl_ord_id: Option<String>,
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
            cl_ord_id: None,
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
    /// Create a new order
    pub async fn create_order(&self, params: CreateOrderParams) -> Result<Order> {
        let url = format!("{}/api/new_order", self.base_url);

        let body = create_order_body(&params);

        let body_str = body.to_string();
        let headers = self.build_auth_headers(Some(&body_str)).await?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        let result = parse_order_response(response, "/api/new_order").await?;

        // Build order from response
        let now = chrono::Utc::now().to_rfc3339();
        let order = Order {
            id: result
                .get("request_id")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string(),
            cl_ord_id: params.cl_ord_id,
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
    pub async fn cancel_order(&self, _symbol: &str, order_id: &str) -> Result<()> {
        let url = format!("{}/api/cancel_order", self.base_url);
        let order_id = order_id.parse::<i64>().map_err(|_| Error::Validation {
            field: "order_id".to_string(),
            message: format!("expected an integer order ID, got '{order_id}'"),
        })?;

        let body = cancel_order_body(order_id);

        let body_str = body.to_string();
        let headers = self.build_auth_headers(Some(&body_str)).await?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        parse_order_response(response, "/api/cancel_order").await?;

        Ok(())
    }

    /// Submit one asynchronous batch cancellation by exchange order IDs.
    pub async fn cancel_orders(&self, order_ids: &[i64]) -> Result<()> {
        if order_ids.is_empty() {
            return Ok(());
        }

        let url = format!("{}/api/cancel_orders", self.base_url);
        let body = cancel_orders_body(order_ids);

        let body_str = body.to_string();
        let headers = self.build_auth_headers(Some(&body_str)).await?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        parse_order_response(response, "/api/cancel_orders").await?;

        Ok(())
    }

    /// Cancel every currently-open order for a symbol using the documented
    /// ID-list batch API. The accepted response is asynchronous; callers that
    /// require a clean book must still confirm through order events or polling.
    pub async fn cancel_all_orders(&self, symbol: &str) -> Result<()> {
        let orders = self.get_open_orders(Some(symbol)).await?;
        let order_ids = orders
            .iter()
            .map(|order| {
                order.id.parse::<i64>().map_err(|_| Error::Validation {
                    field: "order_id".to_string(),
                    message: format!("exchange returned non-integer order ID '{}'", order.id),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        self.cancel_orders(&order_ids).await
    }
}

fn create_order_body(params: &CreateOrderParams) -> serde_json::Value {
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
            TimeInForce::Alo => "alo",
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
    if let Some(value) = &params.cl_ord_id {
        body["cl_ord_id"] = json!(value);
    }
    if let Some(value) = &params.price {
        body["price"] = json!(value);
    }
    if let Some(value) = &params.stop_price {
        body["stop_price"] = json!(value);
    }
    if let Some(value) = &params.sl_price {
        body["sl_price"] = json!(value);
    }
    if let Some(value) = &params.tp_price {
        body["tp_price"] = json!(value);
    }
    body
}

fn cancel_order_body(order_id: i64) -> serde_json::Value {
    json!({ "order_id": order_id })
}

fn cancel_orders_body(order_ids: &[i64]) -> serde_json::Value {
    json!({ "order_id_list": order_ids })
}

async fn parse_order_response(
    response: reqwest::Response,
    endpoint: &str,
) -> Result<serde_json::Value> {
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Api {
            code: status.as_u16(),
            message: text,
            endpoint: Some(endpoint.to_string()),
            retryable: response_code_is_retryable(status.as_u16()),
        });
    }

    let result = response.json::<serde_json::Value>().await?;
    ensure_order_response_success(&result, endpoint)?;
    Ok(result)
}

fn ensure_order_response_success(result: &serde_json::Value, endpoint: &str) -> Result<()> {
    if let Some(code) = result.get("code").and_then(|value| value.as_i64()) {
        if code != 0 {
            let code = u16::try_from(code).unwrap_or(u16::MAX);
            let message = result
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("Order request rejected");
            return Err(Error::Api {
                code,
                message: message.to_string(),
                endpoint: Some(endpoint.to_string()),
                retryable: response_code_is_retryable(code),
            });
        }
    }
    Ok(())
}

fn response_code_is_retryable(code: u16) -> bool {
    code == 429 || code >= 500
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

    #[test]
    fn create_body_includes_client_order_id() {
        let params = CreateOrderParams {
            symbol: "BTC-USD".to_string(),
            cl_ord_id: Some("maker-buy-0-42".to_string()),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: "0.01".to_string(),
            price: Some("65000".to_string()),
            time_in_force: Some(TimeInForce::Alo),
            ..Default::default()
        };

        let body = create_order_body(&params);
        assert_eq!(body["cl_ord_id"], "maker-buy-0-42");
        assert_eq!(body["time_in_force"], "alo");
        assert_eq!(body["reduce_only"], false);
    }

    #[test]
    fn response_codes_classify_rate_limit_as_retryable() {
        assert!(!response_code_is_retryable(400));
        assert!(!response_code_is_retryable(401));
        assert!(response_code_is_retryable(429));
        assert!(response_code_is_retryable(500));
    }

    #[test]
    fn cancellation_bodies_use_documented_order_id_fields() {
        assert_eq!(cancel_order_body(42), serde_json::json!({ "order_id": 42 }));
        assert_eq!(
            cancel_orders_body(&[42, 43]),
            serde_json::json!({ "order_id_list": [42, 43] })
        );
    }

    #[test]
    fn response_body_errors_are_not_treated_as_success() {
        let error = ensure_order_response_success(
            &serde_json::json!({ "code": 429, "message": "rate limited" }),
            "/api/cancel_orders",
        )
        .unwrap_err();
        assert!(error.is_retryable());

        assert!(ensure_order_response_success(
            &serde_json::json!({ "code": 0, "message": "accepted" }),
            "/api/cancel_orders",
        )
        .is_ok());
    }
}
