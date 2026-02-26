//! Output formatting utilities

use crate::models::*;
use tabled::{Table, Tabled};

/// Format data as table
pub fn format_table<T: Tabled>(data: Vec<T>) -> String {
    Table::new(data).to_string()
}

/// Format single item as table
pub fn format_item<T: Tabled>(item: T) -> String {
    Table::new(vec![item]).to_string()
}

/// Format as JSON
pub fn format_json<T: serde::Serialize>(data: &T) -> crate::Result<String> {
    serde_json::to_string_pretty(data).map_err(|e| crate::Error::Json {
        message: e.to_string(),
    })
}

/// Format as CSV (for lists)
pub fn format_csv<T: serde::Serialize>(data: &[T]) -> crate::Result<String> {
    let mut wtr = csv::Writer::from_writer(vec![]);

    for item in data {
        wtr.serialize(item)
            .map_err(|e| crate::Error::Unknown(e.to_string()))?;
    }

    let result = wtr
        .into_inner()
        .map_err(|e| crate::Error::Unknown(e.to_string()))?;

    String::from_utf8(result).map_err(|e| crate::Error::Unknown(e.to_string()))
}

/// Format symbol info for display
impl Tabled for SymbolInfo {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.symbol.clone().into(),
            self.base_asset.clone().into(),
            self.quote_asset.clone().into(),
            self.status.clone().into(),
            format!("{}x", self.max_leverage).into(),
            self.maker_fee.clone().into(),
            self.taker_fee.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Symbol".into(),
            "Base".into(),
            "Quote".into(),
            "Status".into(),
            "Max Lev".into(),
            "Maker Fee".into(),
            "Taker Fee".into(),
        ]
    }
}

/// Format market data for display
impl Tabled for MarketData {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.symbol.clone().into(),
            self.mark_price.clone().into(),
            self.index_price.clone().into(),
            self.last_price.clone().into(),
            self.volume_24h.clone().into(),
            self.high_24h.clone().into(),
            self.low_24h.clone().into(),
            self.funding_rate.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Symbol".into(),
            "Mark Price".into(),
            "Index Price".into(),
            "Last Price".into(),
            "Volume 24h".into(),
            "High 24h".into(),
            "Low 24h".into(),
            "Funding Rate".into(),
        ]
    }
}

/// Format trade for display
impl Tabled for Trade {
    const LENGTH: usize = 100;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.time.split('.').next().unwrap_or(&self.time).into(),
            self.price.clone().into(),
            self.qty.clone().into(),
            if self.is_buyer_taker {
                "Buy".into()
            } else {
                "Sell".into()
            },
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Time".into(),
            "Price".into(),
            "Quantity".into(),
            "Side".into(),
        ]
    }
}

/// Format funding rate for display
impl Tabled for FundingRate {
    const LENGTH: usize = 6;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        vec![
            self.time.split('T').next().unwrap_or(&self.time).into(),
            self.time
                .split('T')
                .nth(1)
                .unwrap_or("")
                .split('.')
                .next()
                .unwrap_or("")
                .into(),
            self.funding_rate.clone().into(),
            self.mark_price.clone().into(),
            self.index_price.clone().into(),
            self.premium.clone().into(),
        ]
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "Date".into(),
            "Time".into(),
            "Funding Rate".into(),
            "Mark Price".into(),
            "Index Price".into(),
            "Premium".into(),
        ]
    }
}

/// Format order book for display
pub fn format_order_book(book: &OrderBook, limit: usize) -> String {
    let mut output = String::new();

    output.push_str(&format!("Order Book: {}\n", book.symbol));
    output.push_str("=============================\n\n");

    // Asks (sell orders) - reversed to show highest ask first
    output.push_str("Asks (Sell):\n");
    output.push_str("Price\t\tQuantity\n");

    let asks_to_show: Vec<_> = book.asks.iter().rev().take(limit).collect();
    for ask in asks_to_show.iter().rev() {
        output.push_str(&format!("{}\t{}\n", ask[0], ask[1]));
    }

    // Spread
    if let Some(spread) = book.spread() {
        output.push_str(&format!("\nSpread: {}\n", spread));
    }

    // Bids (buy orders)
    output.push_str("\nBids (Buy):\n");
    output.push_str("Price\t\tQuantity\n");

    for bid in book.bids.iter().take(limit) {
        output.push_str(&format!("{}\t{}\n", bid[0], bid[1]));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json() {
        let symbol = SymbolInfo {
            symbol: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 9,
            price_tick_decimals: 2,
            qty_tick_decimals: 4,
            min_order_qty: "0.0001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0001".to_string(),
            taker_fee: "0.0004".to_string(),
            status: "trading".to_string(),
        };

        let json = format_json(&symbol).unwrap();
        assert!(json.contains("BTC-USD"));
        assert!(json.contains("\"symbol\""));
    }

    #[test]
    fn test_format_order_book() {
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

        let formatted = format_order_book(&book, 10);
        assert!(formatted.contains("BTC-USD"));
        assert!(formatted.contains("Asks (Sell)"));
        assert!(formatted.contains("Bids (Buy)"));
    }
}
