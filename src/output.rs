//! Output formatting utilities

use crate::models::*;
use tabled::{Table as TabledTable, Tabled};

/// Format data as table
pub fn format_table<T: Tabled>(data: Vec<T>) -> String {
    TabledTable::new(data).to_string()
}

/// Format single item as table
pub fn format_item<T: Tabled>(item: T) -> String {
    TabledTable::new(vec![item]).to_string()
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
    output.push_str(&format!("{:<12} {}\n", "Price", "Quantity"));

    let asks_to_show: Vec<_> = book.asks.iter().rev().take(limit).collect();
    for ask in asks_to_show.iter().rev() {
        output.push_str(&format!("{:<12} {}\n", ask[0], ask[1]));
    }

    // Spread
    if let Some(spread) = book.spread() {
        output.push_str(&format!("\nSpread: {}\n", spread));
    }

    // Bids (buy orders)
    output.push_str("\nBids (Buy):\n");
    output.push_str(&format!("{:<12} {}\n", "Price", "Quantity"));

    for bid in book.bids.iter().take(limit) {
        output.push_str(&format!("{:<12} {}\n", bid[0], bid[1]));
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
}

/// Format dashboard as MVP compact view (Issue #156)
pub fn format_dashboard_mvp(snapshot: &DashboardSnapshot, compact: bool) -> String {
    let mut output = String::new();
    let width = 65;

    // Helper for border
    let border = || format!("┌{}┐\n", "─".repeat(width));
    let sep = || format!("├{}┤\n", "─".repeat(width));
    let footer = || format!("└{}┘\n", "─".repeat(width));

    // Header
    let now = chrono::Utc::now();
    let time_str = now.format("%H:%M").to_string();
    output.push_str(&border());
    output.push_str(&format!(
        "│ standx dashboard refresh: {:<width$} │\n",
        time_str,
        width = width - 6
    ));
    output.push_str(&sep());

    // TICKERS
    let tickers: Vec<String> = snapshot
        .market
        .iter()
        .map(|m| {
            let last: f64 = m.last_price.parse().unwrap_or(0.0);
            let low: f64 = m.low_24h.parse().unwrap_or(0.0);
            let change = if low > 0.0 {
                ((last - low) / low) * 100.0
            } else {
                0.0
            };
            let arrow = if change > 0.0 {
                "▲"
            } else if change < 0.0 {
                "▼"
            } else {
                ""
            };
            format!(
                "{} ${} {}{:.2}%",
                m.symbol,
                m.mark_price,
                arrow,
                change.abs()
            )
        })
        .collect();

    let tickers_str = if tickers.is_empty() {
        "No market data".to_string()
    } else {
        tickers.join(" | ")
    };
    output.push_str(&format!(
        "│ TICKERS: {:<width$} │\n",
        tickers_str,
        width = width - 12
    ));
    output.push_str(&sep());

    // ACCOUNT
    let account_str = if let Some(ref bal) = snapshot.account {
        format!(
            "Total={} Available={} PnL={}",
            bal.balance, bal.cross_available, bal.pnl_24h
        )
    } else {
        "Not authenticated".to_string()
    };
    output.push_str(&format!(
        "│ ACCOUNT: {:<width$} │\n",
        account_str,
        width = width - 12
    ));
    output.push_str(&sep());

    // POSITIONS
    output.push_str("│ POSITIONS:\n");
    if snapshot.positions.is_empty() {
        output.push_str("│   No open positions\n");
    } else {
        for (i, p) in snapshot.positions.iter().enumerate() {
            let side = format!("{:?}", p.side.unwrap_or(crate::models::OrderSide::Buy));
            let pnl_arrow = if p.upnl.parse::<f64>().unwrap_or(0.0) > 0.0 {
                "▲"
            } else {
                "▼"
            };
            let line = format!(
                "#{} {} {} @{} mark={} pnl={} {}",
                i + 1,
                p.symbol,
                side,
                p.entry_price,
                p.mark_price,
                p.upnl,
                pnl_arrow
            );
            output.push_str(&format!("│   {:<width$} │\n", line, width = width - 4));
        }
    }
    output.push_str(&sep());

    // ORDER BOOK + ACTIVE ORDERS
    output.push_str("│ ORDER BOOK:\n");
    // Get order book for first symbol if available
    if let Some(m) = snapshot.market.first() {
        output.push_str(&format!(
            "│   Symbol: {:<width$} │\n",
            m.symbol,
            width = width - 14
        ));
    }

    // Active orders
    output.push_str("│ ACTIVE ORDERS:\n");
    if snapshot.orders.is_empty() {
        output.push_str("│   No open orders\n");
    } else {
        for (i, o) in snapshot.orders.iter().enumerate() {
            let line = format!(
                "#{} {} {:?} {} @{}",
                i + 1,
                o.symbol,
                o.side,
                o.qty,
                o.price
            );
            output.push_str(&format!("│   {:<width$} │\n", line, width = width - 4));
        }
    }

    // RECENT TRADES (skip if compact)
    if !compact {
        output.push_str(&sep());
        output.push_str("│ RECENT TRADES:\n");
        if snapshot.trades.is_empty() {
            output.push_str("│   No recent trades\n");
        } else {
            for t in &snapshot.trades {
                // Format time to HH:MM:SS from ISO format "2026-03-04T02:21:26.633550Z"
                let time_short = if t.time.contains('T') {
                    t.time.split('T').nth(1).unwrap_or(&t.time).split('.').next().unwrap_or(&t.time)
                } else {
                    &t.time
                };
                // Use is_buyer_taker to determine side
                let side = if t.is_buyer_taker { "BUY" } else { "SELL" };
                let line = format!("{} {} {} {}", time_short, t.price, t.qty, side);
                output.push_str(&format!("│   {:<width$} │\n", line, width = width - 4));
            }
        }
    }

    // Footer
    output.push_str(&footer());
    output
}
