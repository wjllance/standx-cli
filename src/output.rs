//! Output formatting utilities

use crate::models::*;
use chrono::Utc;
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

/// Format dashboard in compact three-column layout
pub fn format_dashboard_compact(snapshot: &DashboardSnapshot) -> String {
    let mut output = String::new();
    let col_width = 25;

    // ANSI color codes
    const RED: &str = "\x1B[31m";
    const GREEN: &str = "\x1B[32m";
    const RESET: &str = "\x1B[0m";
    
    // Top border
    output.push_str("┌──────────────────────────────────────────────────────────────┐\n");

    // Status bar with time
    let time = Utc::now().format("%H:%M:%S UTC").to_string();
    output.push_str(&format!("│ STANDX  {:<68}│\n", time));
    output.push_str("│                                                                │\n");
    output.push_str("└──────────────────────────────────────────────────────────────┘\n");

    // Account line (show login prompt if not authenticated)
    if let Some(balance) = &snapshot.account {
        let equity = format_currency(&balance.equity);
        let (pnl_color, pnl_str) = format_pnl_color(&balance.pnl_24h, RED, GREEN, RESET);
        output.push_str(&format!("│ EQUITY: ${:<15} PnL: {}{}{}\n", equity, pnl_color, pnl_str, RESET));
    } else {
        output.push_str("│ Not authenticated - Run 'standx auth login' to view account\n");
    }

    output.push_str("├──────────────────────────────────────────────────────────────┤\n");

    // Fund details (if authenticated)
    if let Some(balance) = &snapshot.account {
        let balance_str = format!("Balance: {}", format_currency(&balance.balance));
        let avail_str = format!("Available: {}", format_currency(&balance.cross_available));
        let locked_str = format!("Locked: {}", format_currency(&balance.locked));

        output.push_str(&format!(
            "│ {:<width$}{:<width$}{:<width$}│\n",
            balance_str,
            avail_str,
            locked_str,
            width = col_width
        ));
    }

    output.push_str("├──────────────────────────────────────────────────────────────┤\n");

    // Fund details
    if let Some(balance) = &snapshot.account {
        let balance_str = format!("Balance: {}", format_currency(&balance.balance));
        let avail_str = format!("Available: {}", format_currency(&balance.cross_available));
        let locked_str = format!("Locked: {}", format_currency(&balance.locked));

        output.push_str(&format!(
            "│ {:<width$}{:<width$}{:<width$}│\n",
            balance_str,
            avail_str,
            locked_str,
            width = col_width
        ));
    }

    output.push_str("├──────────────────────────────────────────────────────────────┤\n");

    // Column headers
    output.push_str(&format!(
        "│ {:<width$}{:<width$}{:<width$}│\n",
        "POSITION",
        "ORDER",
        "MARKET",
        width = col_width
    ));
    output.push_str(&format!(
        "│ {:<width$}{:<width$}{:<width$}│\n",
        "─────────",
        "─────────",
        "─────────",
        width = col_width
    ));

    // Data rows
    let max_rows = 5
        .max(snapshot.positions.len())
        .max(snapshot.orders.len())
        .max(snapshot.market.len());

    for i in 0..max_rows {
        output.push_str("│ ");

        // Position column
        if let Some(pos) = snapshot.positions.get(i) {
            let side = match pos.side {
                Some(OrderSide::Buy) => "LONG",
                Some(OrderSide::Sell) => "SHORT",
                None => "N/A",
            };
            // Calculate risk bar
            let lev = pos.leverage.parse::<f64>().unwrap_or(0.0);
            let (risk_color, risk_bar) = get_risk_bar(lev);
            let risk_str = format!("{}{}x", risk_color, lev as i32);
            
            // Show: SYMBOL QTY @PRICE LEV
            output.push_str(&format!(
                "{:<width$}",
                format!("{} {} @{} {} {}", pos.symbol, pos.qty, pos.entry_price, risk_str, risk_bar),
                width = col_width
            ));
        } else {
            output.push_str(&" ".repeat(col_width));
        }

        // Order column
        if let Some(order) = snapshot.orders.get(i) {
            let order_type = match &order.order_type {
                OrderType::Limit => "LIM",
                OrderType::Market => "MKT",
            };
            let side = format!("{:?}", order.side);
            output.push_str(&format!(
                "{:<width$}",
                format!("{} {} {}", side, order_type, order.symbol),
                width = col_width
            ));
        } else {
            output.push_str(&" ".repeat(col_width));
        }

        // Market column with price change indicator
        if let Some(market) = snapshot.market.get(i) {
            // Calculate 24h change
            let change = calculate_change(&market.last_price, &market.high_24h, &market.low_24h);
            let (color, arrow) = if change > 0.0 {
                (GREEN, "▲")
            } else if change < 0.0 {
                (RED, "▼")
            } else {
                ("", "")
            };
            let change_str = format!("{color}{arrow}{change:.1}%{RESET}", color = color, arrow = arrow, change = change.abs(), RESET = RESET);
            output.push_str(&format!(
                "{:<width$}",
                format!("{} {} {}", market.symbol, format_currency(&market.mark_price), change_str),
                width = col_width
            ));
        } else {
            output.push_str(&" ".repeat(col_width));
        }

        output.push_str("│\n");
    }

    // Bottom border
    output.push_str("└──────────────────────────────────────────────────────────────┘\n");

    output
}

/// Format currency with $ prefix and 2 decimal places
fn format_currency(value: &str) -> String {
    // Try to parse as f64 and format
    if let Ok(v) = value.parse::<f64>() {
        if v >= 1000.0 {
            format!("${:.2}", v)
        } else {
            format!("${:.4}", v)
        }
    } else {
        format!("${}", value)
    }
}

/// Format PnL with color indicator
fn format_pnl(value: &str) -> String {
    if let Ok(v) = value.parse::<f64>() {
        if v > 0.0 {
            format!("+${:.2}", v)
        } else if v < 0.0 {
            format!("-${:.2}", v.abs())
        } else {
            "$0.00".to_string()
        }
    } else {
        value.to_string()
    }
}

/// Format PnL with color indicator - returns (color_code, formatted_value)
fn format_pnl_color(value: &str, red: &str, green: &str, reset: &str) -> (String, String) {
    if let Ok(v) = value.parse::<f64>() {
        if v > 0.0 {
            (green.to_string(), format!("+${:.2}", v))
        } else if v < 0.0 {
            (red.to_string(), format!("-${:.2}", v.abs()))
        } else {
            (String::new(), "$0.00".to_string())
        }
    } else {
        (String::new(), value.to_string())
    }
}

/// Calculate 24h change percentage
fn calculate_change(last: &str, high: &str, low: &str) -> f64 {
    let last = last.parse::<f64>().unwrap_or(0.0);
    let high = high.parse::<f64>().unwrap_or(0.0);
    let low = low.parse::<f64>().unwrap_or(0.0);
    
    if last > 0.0 && high > 0.0 && low > 0.0 {
        // Simple calculation: (last - open) / open * 100
        // Using high/low as proxy for open
        let open = (high + low) / 2.0;
        if open > 0.0 {
            return (last - open) / open * 100.0;
        }
    }
    0.0
}

/// Generate risk bar based on leverage level
/// Returns (color_code, bar_string)
fn get_risk_bar(leverage: f64) -> (&'static str, String) {
    const RED: &str = "\x1B[31m";
    const YELLOW: &str = "\x1B[33m"; 
    const GREEN: &str = "\x1B[32m";
    const RESET: &str = "\x1B[0m";
    
    let total = 10;
    let filled = if leverage >= 50.0 {
        2 // dangerous
    } else if leverage >= 30.0 {
        4 // warning
    } else if leverage >= 10.0 {
        6 // caution
    } else {
        8 // safe
    };
    
    let (color, bar) = if leverage >= 50.0 {
        (RED, "█".repeat(filled) + &"░".repeat(total - filled))
    } else if leverage >= 30.0 {
        (YELLOW, "█".repeat(filled) + &"░".repeat(total - filled))
    } else {
        (GREEN, "█".repeat(filled) + &"░".repeat(total - filled))
    };
    
    (color, bar)
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
