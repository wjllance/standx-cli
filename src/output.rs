//! Output formatting utilities

use crate::models::*;
use chrono::{DateTime, Local, TimeZone, Utc};
use tabled::{Table as TabledTable, Tabled};

fn format_trade_time_short(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }

    // Handle unix timestamps from API/websocket (seconds or milliseconds).
    if let Ok(ts) = raw.parse::<i64>() {
        let dt_utc = if raw.len() >= 13 {
            Utc.timestamp_millis_opt(ts).single()
        } else {
            Utc.timestamp_opt(ts, 0).single()
        };
        if let Some(dt) = dt_utc {
            return dt.with_timezone(&Local).format("%H:%M:%S").to_string();
        }
    }

    // Handle RFC3339-like strings: "2026-03-04T02:21:26.633550Z"
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return dt.with_timezone(&Local).format("%H:%M:%S").to_string();
    }

    // Fallback to the previous best-effort splitter.
    if raw.contains('T') {
        return raw
            .split('T')
            .nth(1)
            .unwrap_or(raw)
            .split('.')
            .next()
            .unwrap_or(raw)
            .to_string();
    }

    raw.to_string()
}

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
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.saturating_sub(2))
        .filter(|v| *v >= 60)
        .unwrap_or(78);

    // Helper for border
    let border = || format!("┌{}┐\n", "─".repeat(width));
    let sep = || format!("├{}┤\n", "─".repeat(width));
    let footer = || format!("└{}┘\n", "─".repeat(width));
    let truncate_pad = |text: &str, target_width: usize| -> String {
        let mut chars: Vec<char> = text.chars().collect();
        if chars.len() > target_width {
            if target_width > 3 {
                chars.truncate(target_width - 3);
                let mut trimmed: String = chars.into_iter().collect();
                trimmed.push_str("...");
                return format!("{:<width$}", trimmed, width = target_width);
            }
            return ".".repeat(target_width);
        }
        format!("{:<width$}", text, width = target_width)
    };
    let fit = |text: &str| -> String { truncate_pad(text, width) };
    let push_line = |out: &mut String, text: &str| {
        out.push_str(&format!("│{}│\n", fit(text)));
    };
    let left_w = (width.saturating_sub(1)) / 2;
    let right_w = width.saturating_sub(1 + left_w);
    let push_two_col = |out: &mut String, left: &str, right: &str| {
        let l = truncate_pad(left, left_w);
        let r = truncate_pad(right, right_w);
        out.push_str(&format!("│{}│{}│\n", l, r));
    };

    // Header
    let now = chrono::Local::now();
    let time_str = now.format("%H:%M:%S").to_string();
    output.push_str(&border());
    let title = " StandX Dashboard";
    let right = format!("REFRESH: {}", time_str);
    let spacing = width.saturating_sub(title.chars().count() + right.chars().count());
    output.push_str(&format!("│{}{}{}│\n", title, " ".repeat(spacing), right));
    output.push_str(&sep());

    // TICKERS
    let ticker_items: Vec<String> = snapshot
        .market
        .iter()
        .map(|m| {
            let change_display = m
                .change_24h_percent
                .parse::<f64>()
                .ok()
                .map(|change| {
                    let arrow = if change > 0.0 {
                        "▲"
                    } else if change < 0.0 {
                        "▼"
                    } else {
                        ""
                    };
                    format!("{} {:.2}%", arrow, change.abs())
                })
                .unwrap_or_else(|| "N/A".to_string());
            format!("{} ${} {}", m.symbol, m.mark_price, change_display)
        })
        .collect();

    push_line(&mut output, " TICKERS:");
    if ticker_items.is_empty() {
        push_line(&mut output, "   No market data");
    } else {
        for row in ticker_items.chunks(2) {
            push_line(&mut output, &format!("   {}", row.join(" | ")));
        }
    }
    output.push_str(&sep());

    // ACCOUNT
    let fmt2 = |v: &str| -> String {
        v.parse::<f64>()
            .map(|n| format!("{:.2}", n))
            .unwrap_or_else(|_| v.to_string())
    };
    let account_str = if let Some(ref bal) = snapshot.account {
        format!(
            "Total={} Available={} uPnL={} PnL={}",
            fmt2(&bal.balance),
            fmt2(&bal.cross_available),
            fmt2(&bal.upnl),
            fmt2(&snapshot.total_realized_pnl)
        )
    } else {
        "Not authenticated".to_string()
    };
    push_line(&mut output, &format!(" ACCOUNT: {}", account_str));
    output.push_str(&sep());

    let fmt_book_price = |v: &str| -> String {
        v.parse::<f64>()
            .map(|n| format!("{:>10.2}", n))
            .unwrap_or_else(|_| format!("{:>10}", v))
    };
    let fmt_book_qty = |v: &str| -> String {
        v.parse::<f64>()
            .map(|n| format!("{:>9.4}", n))
            .unwrap_or_else(|_| format!("{:>9}", v))
    };
    let mut order_book_lines: Vec<String> = Vec::new();
    if let Some(ref ob) = snapshot.order_book {
        order_book_lines.push(format!(" ORDER BOOK ({}):", ob.symbol));
        if ob.asks.is_empty() {
            order_book_lines.push("   No asks".to_string());
        } else {
            for ask in ob.asks.iter().take(3).rev() {
                let price = fmt_book_price(&ask[0]);
                let qty = fmt_book_qty(&ask[1]);
                order_book_lines.push(format!("   {} {} ASK", price, qty));
            }
        }
        order_book_lines.push("   ---- spread ----".to_string());
        if ob.bids.is_empty() {
            order_book_lines.push("   No bids".to_string());
        } else {
            for bid in ob.bids.iter().take(3) {
                let price = fmt_book_price(&bid[0]);
                let qty = fmt_book_qty(&bid[1]);
                order_book_lines.push(format!("   {} {} BID", price, qty));
            }
        }
    } else {
        order_book_lines.push(" ORDER BOOK: unavailable".to_string());
    }

    let mut trade_lines: Vec<String> = Vec::new();
    if !compact {
        trade_lines.push(" RECENT TRADES:".to_string());
        if snapshot.trades.is_empty() {
            trade_lines.push("   No recent trades".to_string());
        } else {
            for t in &snapshot.trades {
                let time_short = format_trade_time_short(&t.time);
                let side = if t.is_buyer_taker { "BUY" } else { "SELL" };
                trade_lines.push(format!("   {} {} {} {}", time_short, t.price, t.qty, side));
            }
        }
    }

    // ORDER BOOK + RECENT TRADES (2-column when enough space)
    if !compact && width >= 66 {
        let max_rows = order_book_lines.len().max(trade_lines.len());
        for i in 0..max_rows {
            let left = order_book_lines.get(i).map_or("", String::as_str);
            let right = trade_lines.get(i).map_or("", String::as_str);
            push_two_col(&mut output, left, right);
        }
        output.push_str(&sep());
    } else {
        for line in &order_book_lines {
            push_line(&mut output, line);
        }
        output.push_str(&sep());
        if !compact {
            for line in &trade_lines {
                push_line(&mut output, line);
            }
            output.push_str(&sep());
        }
    }

    // POSITIONS (moved near bottom)
    push_line(&mut output, " POSITIONS:");
    if snapshot.positions.is_empty() {
        push_line(&mut output, "   No open positions");
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
            push_line(&mut output, &format!("   {}", line));
        }
    }
    output.push_str(&sep());

    // ACTIVE ORDERS (moved near bottom)
    push_line(&mut output, " ACTIVE ORDERS:");
    if snapshot.orders.is_empty() {
        push_line(&mut output, "   No open orders");
    } else {
        for (i, o) in snapshot.orders.iter().enumerate() {
            let side = format!("{:?}", o.side);
            let qty_display = if o.qty.parse::<f64>().unwrap_or(-1.0).abs() < f64::EPSILON {
                "All".to_string()
            } else {
                o.qty.clone()
            };
            let line = format!(
                "#{} {} {} {} @{}",
                i + 1,
                o.symbol,
                side,
                qty_display,
                o.price
            );
            push_line(&mut output, &format!("   {}", line));
        }
    }
    output.push_str(&sep());

    push_line(
        &mut output,
        " Usage: standx dashboard --symbol BTC-USD --watch 5",
    );

    // Footer
    output.push_str(&footer());
    output
}
