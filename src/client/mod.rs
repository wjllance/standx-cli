//! HTTP client for StandX API

use crate::error::{Error, Result};
use crate::models::*;
use reqwest::{Client, ClientBuilder};
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_BASE_URL: &str = "https://perps.standx.com";

/// StandX API client
#[derive(Debug, Clone)]
pub struct StandXClient {
    client: Client,
    base_url: String,
}

impl StandXClient {
    /// Create a new client
    pub fn new() -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Create a new client with custom base URL
    pub fn with_base_url(base_url: String) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self { client, base_url })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ==================== Public API ====================

    /// Get all trading symbols information
    pub async fn get_symbol_info(&self) -> Result<Vec<SymbolInfo>> {
        let url = format!("{}/api/query_symbol_info", self.base_url);
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let data = response.json::<Vec<SymbolInfo>>().await?;
        Ok(data)
    }

    /// Get market data for a symbol
    pub async fn get_symbol_market(&self, symbol: &str) -> Result<MarketData> {
        let url = format!("{}/api/query_symbol_market", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("symbol", symbol)])
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

        let data = response.json::<MarketData>().await?;
        Ok(data)
    }

    /// Get price data for a symbol
    pub async fn get_symbol_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/query_symbol_price", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("symbol", symbol)])
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

        let data = response.json::<PriceData>().await?;
        Ok(data)
    }

    /// Get order book depth for a symbol
    pub async fn get_depth(&self, symbol: &str, limit: Option<u32>) -> Result<OrderBook> {
        let url = format!("{}/api/query_depth", self.base_url);
        let mut query = vec![("symbol", symbol.to_string())];
        
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }

        let response = self.client.get(&url).query(&query).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let mut data = response.json::<OrderBook>().await?;
        // Sort bids and asks
        data.sort_bids();
        data.sort_asks();
        Ok(data)
    }

    /// Get recent trades for a symbol
    pub async fn get_recent_trades(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Trade>> {
        let url = format!("{}/api/query_recent_trades", self.base_url);
        let mut query = vec![("symbol", symbol.to_string())];
        
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }

        let response = self.client.get(&url).query(&query).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let data = response.json::<Vec<Trade>>().await?;
        Ok(data)
    }

    /// Get funding rate for a symbol
    pub async fn get_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/api/query_funding_rates", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("symbol", symbol)])
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

        let data = response.json::<FundingRate>().await?;
        Ok(data)
    }

    /// Get kline data for a symbol
    pub async fn get_kline(
        &self,
        symbol: &str,
        resolution: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<Kline>> {
        let url = format!("{}/api/query_kline", self.base_url);
        let from_str = from.to_string();
        let to_str = to.to_string();
        let query = vec![
            ("symbol", symbol),
            ("resolution", resolution),
            ("from", &from_str),
            ("to", &to_str),
        ];

        let response = self.client.get(&url).query(&query).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let data = response.json::<Vec<Kline>>().await?;
        Ok(data)
    }

    /// Get server time
    pub async fn get_server_time(&self) -> Result<i64> {
        let url = format!("{}/api/query_time", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api {
                code: status.as_u16(),
                message: text,
            });
        }

        let data = response.json::<ServerTime>().await?;
        Ok(data.server_time)
    }

    /// Health check
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/health", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Ok(false);
        }

        let data = response.json::<HealthStatus>().await?;
        Ok(data.status == "ok")
    }
}

impl Default for StandXClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_get_symbol_info() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_info")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"symbol":"BTC-USD","base_asset":"BTC","quote_asset":"DUSD","base_decimals":9,"price_tick_decimals":2,"qty_tick_decimals":4,"min_order_qty":"0.0001","def_leverage":"10","max_leverage":"40","maker_fee":"0.0001","taker_fee":"0.0004","status":"trading"}]"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let symbols = client.get_symbol_info().await.unwrap();
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol, "BTC-USD");
    }

    #[tokio::test]
    async fn test_get_symbol_market() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_market")
            .match_query(mockito::Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"symbol":"BTC-USD","mark_price":"68000.00","index_price":"68001.50","last_price":"68000.00","volume_24h":"1234567.89","high_24h":"69000.00","low_24h":"67000.00","funding_rate":"0.0001","next_funding_time":"2026-02-24T16:00:00Z"}"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let market = client.get_symbol_market("BTC-USD").await.unwrap();
        
        assert_eq!(market.symbol, "BTC-USD");
        assert_eq!(market.mark_price, "68000.00");
    }

    #[tokio::test]
    async fn test_health_check() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/health")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok"}"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let healthy = client.health_check().await.unwrap();
        
        assert!(healthy);
    }

    #[tokio::test]
    async fn test_api_error() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_info")
            .with_status(400)
            .with_body("Invalid request")
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let result = client.get_symbol_info().await;
        
        assert!(matches!(result, Err(Error::Api { code: 400, .. })));
    }
}
