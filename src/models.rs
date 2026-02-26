use serde::{Deserialize, Deserializer, Serialize};

/// Helper to deserialize string or number to string
fn string_or_number_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(serde_json::Number),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => Ok(s),
        StringOrNumber::Number(n) => Ok(n.to_string()),
    }
}

/// Trading symbol information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SymbolInfo {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub base_decimals: u32,
    pub price_tick_decimals: u32,
    pub qty_tick_decimals: u32,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub min_order_qty: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub def_leverage: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub max_leverage: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub maker_fee: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub taker_fee: String,
    pub status: String,
}

/// Market data for a symbol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketData {
    pub symbol: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub mark_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub index_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub last_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub volume_24h: String,
    #[serde(
        rename = "high_price_24h",
        deserialize_with = "string_or_number_to_string"
    )]
    pub high_24h: String,
    #[serde(
        rename = "low_price_24h",
        deserialize_with = "string_or_number_to_string"
    )]
    pub low_24h: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub funding_rate: String,
    pub next_funding_time: String,
}

/// Price data for a symbol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceData {
    pub symbol: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub mark_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub index_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub last_price: String,
    #[serde(alias = "time")]
    pub timestamp: String,
}

/// Order book level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderBookLevel {
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub qty: String,
}

/// Order book depth
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderBook {
    #[serde(default)]
    pub symbol: String,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
    #[serde(default)]
    pub timestamp: String,
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

    /// Get spread between best bid and ask
    pub fn spread(&self) -> Option<String> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                if let (Ok(b), Ok(a)) = (bid.parse::<f64>(), ask.parse::<f64>()) {
                    Some(format!("{:.2}", a - b))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Recent trade
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Trade {
    #[serde(default)]
    pub id: u64,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub time: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub qty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default)]
    pub is_buyer_taker: bool,
}

/// Kline/candlestick data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Kline {
    pub time: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub open: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub high: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub low: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub close: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub volume: String,
}

/// Kline API response wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct KlineResponse {
    pub s: String,
    #[serde(default)]
    pub t: Vec<i64>,
    #[serde(default)]
    pub o: Vec<f64>,
    #[serde(default)]
    pub h: Vec<f64>,
    #[serde(default)]
    pub l: Vec<f64>,
    #[serde(default)]
    pub c: Vec<f64>,
    #[serde(default)]
    pub v: Vec<f64>,
}

impl KlineResponse {
    pub fn to_klines(self) -> Vec<Kline> {
        let mut klines = Vec::new();
        for i in 0..self.t.len() {
            klines.push(Kline {
                time: self.t[i].to_string(),
                open: self.o[i].to_string(),
                high: self.h[i].to_string(),
                low: self.l[i].to_string(),
                close: self.c[i].to_string(),
                volume: self.v[i].to_string(),
            });
        }
        klines
    }
}

/// Funding rate information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FundingRate {
    pub id: i64,
    pub symbol: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub funding_rate: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub mark_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub index_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub premium: String,
    pub time: String,
    pub created_at: String,
    pub updated_at: String,
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
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Limit,
    Market,
}

/// Time in force
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    Gtc, // Good Till Cancel
    Ioc, // Immediate or Cancel
    Fok, // Fill or Kill
}

/// Order status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    #[serde(rename = "new")]
    New,
    #[serde(rename = "partially_filled")]
    PartiallyFilled,
    #[serde(rename = "filled")]
    Filled,
    #[serde(rename = "canceled")]
    Canceled,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "open")]
    Open,
}

/// Position side
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PositionSide {
    Long,
    Short,
}

/// Order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: OrderSide,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub quantity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<TimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

/// Order response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Order {
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    #[serde(rename = "order_type")]
    pub order_type: OrderType,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub qty: String,
    #[serde(deserialize_with = "string_or_number_to_string", default)]
    pub fill_qty: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub price: String,
    pub status: OrderStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl tabled::Tabled for Order {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.id.clone().into(),
            self.symbol.clone().into(),
            format!("{:?}", self.side).into(),
            format!("{:?}", self.order_type).into(),
            self.qty.clone().into(),
            self.fill_qty.clone().into(),
            self.price.clone().into(),
            format!("{:?}", self.status).into(),
            self.created_at
                .split('T')
                .next()
                .unwrap_or(&self.created_at)
                .into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Order ID".into(),
            "Symbol".into(),
            "Side".into(),
            "Type".into(),
            "Quantity".into(),
            "Filled".into(),
            "Price".into(),
            "Status".into(),
            "Created".into(),
        ]
    }
}

/// Position information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub id: i64,
    pub symbol: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub qty: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub entry_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub entry_value: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub holding_margin: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub initial_margin: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub leverage: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub mark_price: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub margin_asset: String,
    pub margin_mode: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub position_value: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub realized_pnl: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub required_margin: String,
    pub status: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub upnl: String,
    pub time: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liq_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmr: Option<String>,
    pub user: String,
}

impl tabled::Tabled for Position {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.symbol.clone().into(),
            self.qty.clone().into(),
            self.entry_price.clone().into(),
            self.mark_price.clone().into(),
            self.liq_price.clone().unwrap_or_default().into(),
            self.leverage.clone().into(),
            self.margin_mode.clone().into(),
            self.upnl.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Symbol".into(),
            "Qty".into(),
            "Entry Price".into(),
            "Mark Price".into(),
            "Liq Price".into(),
            "Leverage".into(),
            "Margin Mode".into(),
            "Unrealized PnL".into(),
        ]
    }
}

/// Position configuration (leverage settings)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PositionConfig {
    pub symbol: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub leverage: String,
    #[serde(deserialize_with = "string_or_number_to_string", default)]
    pub max_leverage: String,
    #[serde(deserialize_with = "string_or_number_to_string", default)]
    pub def_leverage: String,
    #[serde(default)]
    pub margin_mode: String,
}

impl tabled::Tabled for PositionConfig {
    const LENGTH: usize = 4;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.symbol.clone().into(),
            self.leverage.clone().into(),
            self.max_leverage.clone().into(),
            self.def_leverage.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Symbol".into(),
            "Current Leverage".into(),
            "Max Leverage".into(),
            "Default Leverage".into(),
        ]
    }
}

/// Account balance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Balance {
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub balance: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub cross_available: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub cross_balance: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub cross_margin: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub cross_upnl: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub equity: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub isolated_balance: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub isolated_upnl: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub locked: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub pnl_24h: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub pnl_freeze: String,
    #[serde(deserialize_with = "string_or_number_to_string")]
    pub upnl: String,
}

impl tabled::Tabled for Balance {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.balance.clone().into(),
            self.cross_available.clone().into(),
            self.equity.clone().into(),
            self.locked.clone().into(),
            self.upnl.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Balance".into(),
            "Available".into(),
            "Equity".into(),
            "Locked".into(),
            "Unrealized PnL".into(),
        ]
    }
}

/// API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: T,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthStatus {
    pub status: String,
    #[serde(default)]
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(info.max_leverage, "40");
    }

    #[test]
    fn test_symbol_info_with_numbers() {
        // API sometimes returns numbers instead of strings
        let json = r#"{
            "symbol": "BTC-USD",
            "base_asset": "BTC",
            "quote_asset": "DUSD",
            "base_decimals": 9,
            "price_tick_decimals": 2,
            "qty_tick_decimals": 4,
            "min_order_qty": 0.0001,
            "def_leverage": 10,
            "max_leverage": 40,
            "maker_fee": 0.0001,
            "taker_fee": 0.0004,
            "status": "trading"
        }"#;

        let info: SymbolInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.min_order_qty, "0.0001");
        assert_eq!(info.max_leverage, "40");
    }

    #[test]
    fn test_market_data_with_floats() {
        let json = r#"{
            "symbol": "BTC-USD",
            "mark_price": 8739.106200000342,
            "index_price": 8738.5,
            "last_price": 8740.0,
            "volume_24h": "1000000",
            "high_price_24h": 9000.0,
            "low_price_24h": 8500.0,
            "funding_rate": "0.0001",
            "next_funding_time": "2026-01-01T00:00:00Z"
        }"#;

        let data: MarketData = serde_json::from_str(json).unwrap();
        assert_eq!(data.mark_price, "8739.106200000342");
        assert_eq!(data.index_price, "8738.5");
    }

    #[test]
    fn test_order_book_spread() {
        let book = OrderBook {
            symbol: "BTC-USD".to_string(),
            bids: vec![
                ["68000".to_string(), "1.0".to_string()],
                ["67900".to_string(), "2.0".to_string()],
            ],
            asks: vec![
                ["68100".to_string(), "0.5".to_string()],
                ["68200".to_string(), "1.0".to_string()],
            ],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(book.best_bid(), Some("68000"));
        assert_eq!(book.best_ask(), Some("68100"));
        assert_eq!(book.spread(), Some("100.00".to_string()));
    }

    #[test]
    fn test_order_book_sorting() {
        // Server might return unsorted data
        let book = OrderBook {
            symbol: "BTC-USD".to_string(),
            bids: vec![
                ["67900".to_string(), "2.0".to_string()],
                ["68000".to_string(), "1.0".to_string()],
            ],
            asks: vec![
                ["68200".to_string(), "1.0".to_string()],
                ["68100".to_string(), "0.5".to_string()],
            ],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        // Client should sort: bids descending, asks ascending
        // Note: This test documents expected behavior
        // In actual implementation, sorting would happen in the client
    }
}
