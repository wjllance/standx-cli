use serde::{Deserialize, Serialize};

/// Trading symbol information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SymbolInfo {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub base_decimals: u32,
    pub price_tick_decimals: u32,
    pub qty_tick_decimals: u32,
    pub min_order_qty: String,
    pub def_leverage: String,
    pub max_leverage: String,
    pub maker_fee: String,
    pub taker_fee: String,
    pub status: String,
}

/// Market data for a symbol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketData {
    pub symbol: String,
    pub mark_price: String,
    pub index_price: String,
    pub last_price: String,
    pub volume_24h: String,
    pub high_24h: String,
    pub low_24h: String,
    pub funding_rate: String,
    pub next_funding_time: String,
}

/// Price data for a symbol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceData {
    pub symbol: String,
    pub mark_price: String,
    pub index_price: String,
    pub last_price: String,
    pub timestamp: String,
}

/// Order book depth
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderBook {
    pub symbol: String,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
    pub timestamp: String,
}

/// Trade information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Trade {
    pub symbol: String,
    pub price: String,
    pub qty: String,
    pub quote_qty: String,
    pub is_buyer_taker: bool,
    pub time: String,
}

/// Funding rate information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FundingRate {
    pub symbol: String,
    pub funding_rate: String,
    pub next_funding_time: String,
}

/// Kline/Candlestick data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Kline {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

/// Server time response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerTime {
    pub server_time: i64,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthStatus {
    pub status: String,
}

/// Order side
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
}

/// Time in force
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Alo,
}

/// Order status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Open,
    Canceled,
    Filled,
    Rejected,
    Untriggered,
}

/// Margin mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MarginMode {
    Cross,
    Isolated,
}

impl OrderBook {
    /// Get best bid price
    pub fn best_bid(&self) -> Option<&str> {
        self.bids.first().map(|b| b[0].as_str())
    }

    /// Get best ask price
    pub fn best_ask(&self) -> Option<&str> {
        self.asks.first().map(|a| a[0].as_str())
    }

    /// Get spread
    pub fn spread(&self) -> Option<String> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                let b: f64 = bid.parse().ok()?;
                let a: f64 = ask.parse().ok()?;
                Some(format!("{:.2}", a - b))
            }
            _ => None,
        }
    }

    /// Sort bids in descending order (best bid first)
    pub fn sort_bids(&mut self) {
        self.bids.sort_by(|a, b| {
            let a_price: f64 = a[0].parse().unwrap_or(0.0);
            let b_price: f64 = b[0].parse().unwrap_or(0.0);
            b_price.partial_cmp(&a_price).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Sort asks in ascending order (best ask first)
    pub fn sort_asks(&mut self) {
        self.asks.sort_by(|a, b| {
            let a_price: f64 = a[0].parse().unwrap_or(0.0);
            let b_price: f64 = b[0].parse().unwrap_or(0.0);
            a_price.partial_cmp(&b_price).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_book_sorting() {
        let mut book = OrderBook {
            symbol: "BTC-USD".to_string(),
            bids: vec![
                ["68000".to_string(), "1.0".to_string()],
                ["68100".to_string(), "0.5".to_string()],
                ["67900".to_string(), "2.0".to_string()],
            ],
            asks: vec![
                ["68200".to_string(), "1.5".to_string()],
                ["68150".to_string(), "0.8".to_string()],
                ["68300".to_string(), "1.2".to_string()],
            ],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        book.sort_bids();
        assert_eq!(book.bids[0][0], "68100");
        assert_eq!(book.bids[2][0], "67900");

        book.sort_asks();
        assert_eq!(book.asks[0][0], "68150");
        assert_eq!(book.asks[2][0], "68300");
    }

    #[test]
    fn test_order_book_spread() {
        let book = OrderBook {
            symbol: "BTC-USD".to_string(),
            bids: vec![["68000".to_string(), "1.0".to_string()]],
            asks: vec![["68100".to_string(), "1.0".to_string()]],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(book.spread(), Some("100.00".to_string()));
    }

    #[test]
    fn test_symbol_info_deserialization() {
        let json = r#"{
            "symbol": "BTC-USD",
            "base_asset": "BTC",
            "quote_asset": "DUSD",
            "base_decimals": 9,
            "price_tick_decimals": 2,
            "qty_tick_decimals": 4,
            "min_order_qty": "0.0001",
            "def_leverage": "10",
            "max_leverage": "40",
            "maker_fee": "0.0001",
            "taker_fee": "0.0004",
            "status": "trading"
        }"#;

        let info: SymbolInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.symbol, "BTC-USD");
        assert_eq!(info.base_asset, "BTC");
        assert_eq!(info.status, "trading");
    }
}
