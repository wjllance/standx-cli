//! Account API client methods

use crate::auth::{Credentials, StandXSigner};
use crate::client::StandXClient;
use crate::error::{Error, Result};
use crate::models::{Balance, Order, Position};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

/// API response wrapper for list endpoints
#[derive(Debug, Deserialize)]
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
            return Err(Error::AuthRequired);
        }

        let mut headers = HeaderMap::new();
        
        // Authorization header with JWT
        let auth_value = format!("Bearer {}", creds.token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|e| Error::Api { code: 500, message: e.to_string() })?
        );
        
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        
        Ok(headers)
    }

    /// Sign request with Ed25519
    fn sign_request(
        &self,
        payload: &str,
    ) -> Result<(StandXSigner, crate::auth::RequestSignature)> {
        let creds = Credentials::load()?;
        let signer = StandXSigner::from_base58(
            &creds.private_key
        ).map_err(|_| Error::InvalidCredentials)?;
        
        let signature = signer.sign_request_now(payload);
        Ok((signer, signature))
    }

    /// Get account balances
    pub async fn get_balance(
        &self,
    ) -> Result<Balance> {
        let url = format!("{}/api/query_balance", self.base_url);
        let headers = self.auth_headers()?;
        
        let response = self.client
            .get(&url)
            .headers(headers)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let data = response.json::<Balance>().await?;
        Ok(data)
    }

    /// Get positions
    pub async fn get_positions(
        &self,
        symbol: Option<&str>,
    ) -> Result<Vec<Position>> {
        let url = format!("{}/api/query_positions", self.base_url);
        let headers = self.auth_headers()?;
        
        let mut query = vec![];
        if let Some(s) = symbol {
            query.push(("symbol", s));
        }

        let response = self.client
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
            });
        }

        let wrapper = response.json::<ApiListResponse<Position>>().await?;
        Ok(wrapper.result)
    }

    /// Get open orders
    pub async fn get_open_orders(
        &self,
        symbol: Option<&str>,
    ) -> Result<Vec<Order>> {
        let url = format!("{}/api/query_open_orders", self.base_url);
        let headers = self.auth_headers()?;
        
        let mut query = vec![];
        if let Some(s) = symbol {
            query.push(("symbol", s));
        }

        let response = self.client
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
        let url = format!("{}/api/query_order_history", self.base_url);
        let headers = self.auth_headers()?;
        
        let mut query: Vec<(&str, String)> = vec![];
        if let Some(s) = symbol {
            query.push(("symbol", s.to_string()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }

        let response = self.client
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
            });
        }

        let wrapper = response.json::<ApiListResponse<Order>>().await?;
        Ok(wrapper.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_get_balance_unauthorized() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_balance")
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        // Should fail because no credentials
        let result = client.get_balance().await;
        assert!(result.is_err());
    }
}
