//! Command implementations

use crate::cli::*;
use anyhow::Result;
use standx_cli::auth::Credentials;
use standx_cli::client::order::CreateOrderParams;
use standx_cli::client::StandXClient;
use standx_cli::config::Config;
use standx_cli::models::{DashboardSnapshot, OrderSide, OrderType, TimeInForce};
use standx_cli::output;
use standx_cli::websocket::{StandXWebSocket, WsMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal;

/// Parse time string to timestamp
/// Supports:
/// - Unix timestamp (e.g., "1704067200")
/// - ISO date (e.g., "2024-01-01")
/// - Relative time (e.g., "1h", "1d", "7d", "30m")
pub fn parse_time_string(time_str: &str, default_now: bool) -> anyhow::Result<i64> {
    // Try parsing as unix timestamp first
    if let Ok(timestamp) = time_str.parse::<i64>() {
        return Ok(timestamp);
    }

    // Try parsing as ISO date (YYYY-MM-DD)
    if let Ok(date) = chrono::NaiveDate::parse_from_str(time_str, "%Y-%m-%d") {
        let datetime = date.and_hms_opt(0, 0, 0).unwrap();
        return Ok(datetime.and_utc().timestamp());
    }

    // Try parsing as relative time
    let time_str = time_str.to_lowercase();
    let now = chrono::Utc::now().timestamp();

    if let Some(captures) = regex::Regex::new(r"^(\d+)([smhdw])$")?.captures(&time_str) {
        let value: i64 = captures[1].parse()?;
        let unit = &captures[2];

        let seconds = match unit {
            "s" => value,
            "m" => value * 60,
            "h" => value * 3600,
            "d" => value * 86400,
            "w" => value * 604800,
            _ => return Err(anyhow::anyhow!("Invalid time unit: {}", unit)),
        };

        if default_now {
            // For "to" time, we use now + offset (future) or just now
            return Ok(now + seconds);
        } else {
            // For "from" time, we use now - offset (past)
            return Ok(now - seconds);
        }
    }

    Err(anyhow::anyhow!(
        "Invalid time format: {}. Use unix timestamp, YYYY-MM-DD, or relative like 1h, 1d, 7d",
        time_str
    ))
}

/// Handle order commands
pub async fn handle_order(command: OrderCommands) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        OrderCommands::Create {
            symbol,
            side,
            order_type,
            qty,
            price,
            tif,
            reduce_only,
            sl_price,
            tp_price,
        } => {
            // Parse side
            let side = match side.to_lowercase().as_str() {
                "buy" => OrderSide::Buy,
                "sell" => OrderSide::Sell,
                _ => return Err(anyhow::anyhow!("Invalid side: {}", side)),
            };

            // Parse order type
            let order_type = match order_type.to_lowercase().as_str() {
                "limit" => OrderType::Limit,
                "market" => OrderType::Market,
                _ => return Err(anyhow::anyhow!("Invalid order type: {}", order_type)),
            };

            // Parse time in force
            let time_in_force = tif.map(|t| match t.to_uppercase().as_str() {
                "GTC" => TimeInForce::Gtc,
                "IOC" => TimeInForce::Ioc,
                "FOK" => TimeInForce::Fok,
                _ => TimeInForce::Gtc,
            });

            let params = CreateOrderParams {
                symbol,
                side,
                order_type,
                quantity: qty,
                price,
                time_in_force,
                reduce_only,
                stop_price: None,
                sl_price,
                tp_price,
            };

            let order = client.create_order(params).await?;
            println!("✅ Order created successfully!");
            println!("   Order ID: {}", order.id);
            println!("   Symbol: {}", order.symbol);
            println!("   Side: {:?}", order.side);
            println!("   Type: {:?}", order.order_type);
            println!("   Quantity: {}", order.qty);
            if !order.price.is_empty() && order.price != "0" {
                println!("   Price: {}", order.price);
            }
        }
        OrderCommands::Cancel { symbol, order_id } => {
            client.cancel_order(&symbol, &order_id).await?;
            println!("✅ Order {} cancelled successfully", order_id);
        }
        OrderCommands::CancelAll { symbol } => {
            client.cancel_all_orders(&symbol).await?;
            println!("✅ All orders for {} cancelled successfully", symbol);
        }
    }
    Ok(())
}

/// Handle trade commands
pub async fn handle_trade(command: TradeCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        TradeCommands::History {
            symbol,
            from,
            to,
            limit,
        } => {
            // Parse time parameters with defaults
            let now = chrono::Utc::now().timestamp();
            let from_ts = match from {
                Some(f) => parse_time_string(&f, false)?,
                None => now - 86400, // Default: 1 day ago
            };
            let to_ts = match to {
                Some(t) => parse_time_string(&t, true)?,
                None => now, // Default: now
            };

            let trades = client
                .get_user_trades(&symbol, from_ts, to_ts, limit)
                .await?;

            match output_format {
                OutputFormat::Table => {
                    if trades.is_empty() {
                        println!(
                            "ℹ️  No trades found for {} in the specified time range",
                            symbol
                        );
                    } else {
                        println!("{}", output::format_table(trades));
                    }
                }
                OutputFormat::Json => println!("{}", output::format_json(&trades)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&trades)?),
                OutputFormat::Quiet => {}
            }
        }
    }
    Ok(())
}

/// Handle leverage commands
pub async fn handle_leverage(command: LeverageCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        LeverageCommands::Get { symbol } => {
            // Try to get position config first, fallback to symbol info
            match client.get_position_config(&symbol).await {
                Ok(config) => match output_format {
                    OutputFormat::Table => println!("{}", output::format_item(config)),
                    OutputFormat::Json => println!("{}", output::format_json(&config)?),
                    OutputFormat::Csv => println!("{}", output::format_csv(&[config])?),
                    OutputFormat::Quiet => println!("{}", config.leverage),
                },
                Err(_) => {
                    // Fallback: get leverage from symbol info
                    let symbol_info = client
                        .get_symbol_info()
                        .await?
                        .into_iter()
                        .find(|s| s.symbol == symbol)
                        .ok_or_else(|| anyhow::anyhow!("Symbol {} not found", symbol))?;

                    let config = standx_cli::models::PositionConfig {
                        symbol: symbol.clone(),
                        leverage: symbol_info.def_leverage.clone(),
                        max_leverage: symbol_info.max_leverage.clone(),
                        def_leverage: symbol_info.def_leverage,
                        margin_mode: "cross".to_string(),
                    };

                    match output_format {
                        OutputFormat::Table => println!("{}", output::format_item(config)),
                        OutputFormat::Json => println!("{}", output::format_json(&config)?),
                        OutputFormat::Csv => println!("{}", output::format_csv(&[config])?),
                        OutputFormat::Quiet => println!("{}", config.leverage),
                    }
                }
            }
        }
        LeverageCommands::Set { symbol, leverage } => {
            let leverage_val: u32 = leverage
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid leverage value: {}", leverage))?;

            match client.change_leverage(&symbol, leverage_val).await {
                Ok(_) => println!("✅ Leverage for {} set to {}x", symbol, leverage),
                Err(e) => {
                    println!("⚠️  Leverage change failed");
                    println!("   Symbol: {}", symbol);
                    println!("   Requested leverage: {}x", leverage);
                    println!("   Error: {}", e);
                }
            }
        }
    }
    Ok(())
}

/// Handle margin commands
pub async fn handle_margin(command: MarginCommands) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        MarginCommands::Transfer {
            symbol,
            amount,
            direction,
        } => match client.transfer_margin(&symbol, &amount, &direction).await {
            Ok(_) => {
                println!(
                    "✅ Margin transferred for {}: {} (direction: {})",
                    symbol, amount, direction
                );
            }
            Err(e) => {
                println!("⚠️  Margin transfer failed");
                println!("   Symbol: {}", symbol);
                println!("   Amount: {}", amount);
                println!("   Direction: {}", direction);
                println!("   Error: {}", e);
            }
        },
        MarginCommands::Mode { symbol, set } => {
            if let Some(mode) = set {
                // Set margin mode
                match client.change_margin_mode(&symbol, &mode).await {
                    Ok(_) => println!("✅ Margin mode for {} set to {}", symbol, mode),
                    Err(e) => {
                        println!("⚠️  Margin mode change failed");
                        println!("   Symbol: {}", symbol);
                        println!("   Mode: {}", mode);
                        println!("   Error: {}", e);
                    }
                }
            } else {
                // Get margin mode from position config
                let config = client.get_position_config(&symbol).await?;
                let mode = if config.margin_mode.is_empty() {
                    "cross"
                } else {
                    &config.margin_mode
                };
                println!(
                    "Margin mode for {}: {} (leverage: {}x)",
                    symbol, mode, config.leverage
                );
            }
        }
    }
    Ok(())
}

/// Handle account commands
pub async fn handle_account(command: AccountCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        AccountCommands::Balances => {
            let balance = client.get_balance().await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_item(balance)),
                OutputFormat::Json => println!("{}", output::format_json(&balance)?),
                OutputFormat::Csv => println!("CSV format not supported for single item"),
                OutputFormat::Quiet => {}
            }
        }
        AccountCommands::Positions { symbol } => {
            let positions = client.get_positions(symbol.as_deref()).await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(positions)),
                OutputFormat::Json => println!("{}", output::format_json(&positions)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&positions)?),
                OutputFormat::Quiet => {}
            }
        }
        AccountCommands::Orders { symbol } => {
            let orders = client.get_open_orders(symbol.as_deref()).await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(orders)),
                OutputFormat::Json => println!("{}", output::format_json(&orders)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&orders)?),
                OutputFormat::Quiet => {}
            }
        }
        AccountCommands::History { symbol, limit } => {
            let orders = client
                .get_order_history(symbol.as_deref(), Some(limit))
                .await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(orders)),
                OutputFormat::Json => println!("{}", output::format_json(&orders)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&orders)?),
                OutputFormat::Quiet => {}
            }
        }
        AccountCommands::Config { symbol } => {
            println!("Position config for {} not yet implemented", symbol);
        }
    }
    Ok(())
}

/// Handle config commands
pub async fn handle_config(command: ConfigCommands, output_format: OutputFormat) -> Result<()> {
    match command {
        ConfigCommands::Init => {
            let config = Config::default();
            config.save()?;
            match output_format {
                OutputFormat::Json => {
                    let json = serde_json::json!({
                        "status": "success",
                        "message": "Configuration initialized",
                        "config_file": config.config_file()
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                OutputFormat::Quiet => {}
                _ => println!("✅ Configuration initialized at {:?}", config.config_file()),
            }
        }
        ConfigCommands::Set { key, value } => {
            let mut config = Config::load().unwrap_or_default();
            config.set(&key, &value)?;
            match output_format {
                OutputFormat::Json => {
                    let json = serde_json::json!({
                        "status": "success",
                        "key": key,
                        "value": value
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                OutputFormat::Quiet => {}
                _ => println!("✅ Set {} = {}", key, value),
            }
        }
        ConfigCommands::Get { key } => {
            let config = Config::load().unwrap_or_default();
            let value = config.get(&key)?;
            match output_format {
                OutputFormat::Json => {
                    let json = serde_json::json!({
                        "key": key,
                        "value": value
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                OutputFormat::Quiet => println!("{}", value),
                _ => println!("{}: {}", key, value),
            }
        }
        ConfigCommands::Show => {
            let config = Config::load().unwrap_or_default();
            match output_format {
                OutputFormat::Json => {
                    let json = serde_json::json!({
                        "base_url": config.base_url,
                        "output_format": config.output_format,
                        "default_symbol": config.default_symbol
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                OutputFormat::Quiet => {}
                _ => {
                    println!("Configuration:");
                    println!("  base_url: {}", config.base_url);
                    println!("  output_format: {}", config.output_format);
                    println!("  default_symbol: {}", config.default_symbol);
                }
            }
        }
    }
    Ok(())
}

/// Handle auth commands
pub async fn handle_auth(command: AuthCommands) -> Result<()> {
    match command {
        AuthCommands::Login {
            token,
            token_file,
            private_key,
            key_file,
            interactive,
        } => {
            // Get token
            let token = if interactive || (token.is_none() && token_file.is_none()) {
                println!("Enter JWT Token:");
                rpassword::prompt_password("Token: ")?.trim().to_string()
            } else if let Some(file) = token_file {
                std::fs::read_to_string(file)?.trim().to_string()
            } else {
                token.unwrap()
            };

            // Get private key - always interactive if not provided via file or arg
            let private_key = if let Some(key) = private_key {
                // Provided via --private-key flag
                Some(key)
            } else if let Some(file) = key_file {
                // Provided via --key-file flag
                let key = std::fs::read_to_string(file)?.trim().to_string();
                if key.is_empty() {
                    None
                } else {
                    Some(key)
                }
            } else {
                // Interactive prompt - always ask, but allow empty (optional)
                println!("\nEnter Ed25519 Private Key (Base58) - optional, press Enter to skip:");
                let key = rpassword::prompt_password("Private Key: ")?
                    .trim()
                    .to_string();
                if key.is_empty() {
                    None
                } else {
                    Some(key)
                }
            };

            let credentials = Credentials::new(token, private_key.clone());
            let expires_at = credentials.expires_at_string();
            credentials.save()?;

            println!("✅ Login successful!");
            println!("   Token expires at: {}", expires_at);
            if private_key.is_none() {
                println!("   ⚠️  No private key provided - trading operations will be unavailable");
                println!("   Run 'standx auth login' again to add a private key");
            }
        }
        AuthCommands::Logout => {
            Credentials::delete()?;
            println!("✅ Logged out successfully");
        }
        AuthCommands::Status => match Credentials::load() {
            Ok(creds) => {
                let expires_at = creds.expires_at_string();
                println!("✅ Authenticated");
                println!("   Token expires at: {}", expires_at);
                let remaining = creds.remaining_seconds();
                if remaining < 24 * 60 * 60 {
                    println!("   ⚠️  Warning: Token expires in less than 24 hours!");
                } else {
                    println!("   Remaining: {} hours", remaining / 3600);
                }
                if creds.is_expired() {
                    println!("   ❌ Token has expired! Please login again.");
                }
            }
            Err(_) => {
                println!("❌ Not authenticated");
                println!("   Run 'standx auth login' to authenticate");
            }
        },
    }
    Ok(())
}

/// Handle market commands
pub async fn handle_market(command: MarketCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        MarketCommands::Symbols => {
            let symbols = client.get_symbol_info().await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(symbols)),
                OutputFormat::Json => println!("{}", output::format_json(&symbols)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&symbols)?),
                OutputFormat::Quiet => {}
            }
        }
        MarketCommands::Ticker { symbol } => {
            let ticker = client.get_symbol_market(&symbol).await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_item(ticker)),
                OutputFormat::Json => println!("{}", output::format_json(&ticker)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&[ticker])?),
                OutputFormat::Quiet => println!("{}", ticker.last_price),
            }
        }
        MarketCommands::Tickers => {
            let symbols = client.get_symbol_info().await?;
            let mut tickers = vec![];

            for symbol_info in symbols {
                match client.get_symbol_market(&symbol_info.symbol).await {
                    Ok(ticker) => tickers.push(ticker),
                    Err(e) => eprintln!("Warning: Failed to get {}: {}", symbol_info.symbol, e),
                }
            }

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(tickers)),
                OutputFormat::Json => println!("{}", output::format_json(&tickers)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&tickers)?),
                OutputFormat::Quiet => {}
            }
        }
        MarketCommands::Trades { symbol, limit } => {
            let trades = client.get_recent_trades(&symbol, limit).await?;

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(trades)),
                OutputFormat::Json => println!("{}", output::format_json(&trades)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&trades)?),
                OutputFormat::Quiet => {}
            }
        }
        MarketCommands::Depth { symbol, limit } => {
            let book = client.get_depth(&symbol, limit).await?;

            match output_format {
                OutputFormat::Table => println!(
                    "{}",
                    output::format_order_book(&book, limit.unwrap_or(10) as usize)
                ),
                OutputFormat::Json => println!("{}", output::format_json(&book)?),
                OutputFormat::Csv => println!("CSV format not supported for order book"),
                OutputFormat::Quiet => {
                    if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                        println!("{} {}", bid, ask);
                    }
                }
            }
        }
        MarketCommands::Kline {
            symbol,
            resolution,
            from,
            to,
            limit,
        } => {
            // Parse time parameters
            let now = chrono::Utc::now().timestamp();
            let from_ts = match from {
                Some(f) => parse_time_string(&f, false)?,
                None => now - 86400, // Default: 1 day ago
            };
            let to_ts = match to {
                Some(t) => parse_time_string(&t, false)?, // Fix: use false to get past time
                None => now,                              // Default: now
            };

            let klines = client
                .get_kline(&symbol, &resolution, from_ts, to_ts)
                .await?;

            // Apply limit if specified
            let klines: Vec<_> = if let Some(lim) = limit {
                klines.into_iter().take(lim as usize).collect()
            } else {
                klines
            };

            match output_format {
                OutputFormat::Table => {
                    println!("Kline data for {} ({}):", symbol, resolution);
                    for kline in klines {
                        println!(
                            "  {}: O:{} H:{} L:{} C:{} V:{}",
                            kline.time,
                            kline.open,
                            kline.high,
                            kline.low,
                            kline.close,
                            kline.volume
                        );
                    }
                }
                OutputFormat::Json => println!("{}", output::format_json(&klines)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&klines)?),
                OutputFormat::Quiet => {}
            }
        }
        MarketCommands::Funding { symbol, days } => {
            let now = chrono::Utc::now().timestamp();
            let start_time = now - days * 24 * 60 * 60;
            let funding_rates = client.get_funding_rate(&symbol, start_time, now).await?;

            if funding_rates.is_empty() {
                println!(
                    "ℹ️  No funding rate data available for {} in the last {} days",
                    symbol, days
                );
                println!("   This may be because:");
                println!("   - The symbol is not actively trading");
                println!("   - Funding rates are only recorded at specific intervals");
                println!(
                    "   - Try checking the current funding rate with: standx market ticker {}",
                    symbol
                );
            } else {
                match output_format {
                    OutputFormat::Table => println!("{}", output::format_table(funding_rates)),
                    OutputFormat::Json => println!("{}", output::format_json(&funding_rates)?),
                    OutputFormat::Csv => println!("{}", output::format_csv(&funding_rates)?),
                    OutputFormat::Quiet => {}
                }
            }
        }
    }
    Ok(())
}

/// Handle stream commands
pub async fn handle_stream(command: StreamCommands, verbose: bool) -> Result<()> {
    match command {
        // Public channels - no auth required
        StreamCommands::Price { symbol } => {
            let ws = StandXWebSocket::without_auth_with_verbose(verbose)?;
            let _ = ws.subscribe("price", Some(&symbol)).await;
            let mut rx = ws.connect().await?;

            println!("Streaming price for {}", symbol);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Price(data) = msg {
                    println!(
                        "{} | Mark: {} | Index: {} | Last: {}",
                        data.timestamp, data.mark_price, data.index_price, data.last_price
                    );
                }
            }
        }
        StreamCommands::Depth { symbol, levels } => {
            let ws = StandXWebSocket::without_auth_with_verbose(verbose)?;
            let _ = ws.subscribe("depth_book", Some(&symbol)).await;
            let mut rx = ws.connect().await?;

            println!("Streaming depth for {} (top {} levels)", symbol, levels);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Depth(data) = msg {
                    println!("\n=== Order Book: {} ===", data.symbol);
                    println!("Asks:");
                    for ask in data.asks.iter().take(levels) {
                        println!("  {}: {}", ask[0], ask[1]);
                    }
                    println!("Bids:");
                    for bid in data.bids.iter().take(levels) {
                        println!("  {}: {}", bid[0], bid[1]);
                    }
                }
            }
        }
        StreamCommands::Trade { symbol } => {
            let ws = StandXWebSocket::without_auth_with_verbose(verbose)?;
            let _ = ws.subscribe("public_trade", Some(&symbol)).await;
            let mut rx = ws.connect().await?;

            println!("Streaming trades for {}", symbol);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Trade(data) = msg {
                    let side = data.side.as_deref().unwrap_or({
                        if data.is_buyer_taker {
                            "buy"
                        } else {
                            "sell"
                        }
                    });
                    let side_emoji = match side.to_lowercase().as_str() {
                        "buy" => "🟢 BUY",
                        "sell" => "🔴 SELL",
                        _ => side,
                    };
                    println!(
                        "{} | {} | Price: {} | Qty: {}",
                        data.time, side_emoji, data.price, data.qty
                    );
                }
            }
        }
        // User-level authenticated channels
        StreamCommands::Order => {
            let ws = StandXWebSocket::new_with_verbose(verbose)?;
            let _ = ws.subscribe("order", None).await;
            let mut rx = ws.connect().await?;

            println!("Streaming order updates");
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Order(data) = msg {
                    println!("Order update: {}", serde_json::to_string(&data)?);
                }
            }
        }
        StreamCommands::Position => {
            let ws = StandXWebSocket::new_with_verbose(verbose)?;
            let _ = ws.subscribe("position", None).await;
            let mut rx = ws.connect().await?;

            println!("Streaming position updates");
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Position(data) = msg {
                    println!("Position update: {}", serde_json::to_string(&data)?);
                }
            }
        }
        StreamCommands::Balance => {
            let ws = StandXWebSocket::new_with_verbose(verbose)?;
            let _ = ws.subscribe("balance", None).await;
            let mut rx = ws.connect().await?;

            println!("Streaming balance updates");
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Balance(data) = msg {
                    println!("Balance update: {}", serde_json::to_string(&data)?);
                }
            }
        }
        StreamCommands::Fills => {
            let ws = StandXWebSocket::new_with_verbose(verbose)?;
            let _ = ws.subscribe("trade", None).await;
            let mut rx = ws.connect().await?;

            println!("Streaming fill/trade updates");
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Trade(data) = msg {
                    let side = data.side.as_deref().unwrap_or({
                        if data.is_buyer_taker {
                            "buy"
                        } else {
                            "sell"
                        }
                    });
                    println!(
                        "Fill | {} | Price: {} | Qty: {}",
                        side.to_uppercase(),
                        data.price,
                        data.qty
                    );
                }
            }
        }
    }

    Ok(())
}

/// Handle dashboard commands - unified view of account, positions, orders, and market data
pub async fn handle_dashboard(
    command: DashboardCommands,
    output_format: OutputFormat,
) -> Result<()> {
    match command {
        DashboardCommands::Snapshot {
            symbols,
            verbose,
            watch,
        } => {
            // Build list of symbols to track
            let symbol_list: Vec<String> = if let Some(s) = symbols {
                s.split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec![]
            };

            // Create flag for watch mode interruption
            let should_stop = Arc::new(AtomicBool::new(false));
            let should_stop_clone = should_stop.clone();

            // Set up Ctrl+C handler for watch mode
            if watch.is_some() {
                tokio::spawn(async move {
                    signal::ctrl_c().await.ok();
                    should_stop_clone.store(true, Ordering::SeqCst);
                });
            }

            // Watch mode loop
            if let Some(interval_secs) = watch {
                loop {
                    if should_stop.load(Ordering::SeqCst) {
                        println!("\n👋 Stopping watch mode");
                        break;
                    }

                    // Clear screen for better watch mode experience
                    print!("\x1B[2J\x1B[1H");
                    println!("=== Dashboard Snapshot (refresh: {}s) ===", interval_secs);
                    println!(
                        "Time: {}",
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    println!();

                    // Fetch and display dashboard
                    fetch_and_display_dashboard(&symbol_list, verbose, output_format).await?;

                    // Sleep until next refresh
                    tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;
                }
            } else {
                // Single snapshot mode
                fetch_and_display_dashboard(&symbol_list, verbose, output_format).await?;
            }
        }
    }
    Ok(())
}

/// Fetch and display dashboard data with optional symbol filtering
async fn fetch_and_display_dashboard(
    symbol_filter: &[String],
    verbose: bool,
    output_format: OutputFormat,
) -> Result<()> {
    let client = StandXClient::new()?;

    // Determine which symbols to track
    let symbol_list: Vec<String> = if !symbol_filter.is_empty() {
        symbol_filter.to_vec()
    } else {
        // Default: get all positions' symbols
        let positions = client.get_positions(None).await?;
        positions.into_iter().map(|p| p.symbol).collect()
    };

    // Fetch all data in parallel
    let account = client.get_balance().await.ok();
    let all_positions = client.get_positions(None).await?;
    let all_orders = client.get_open_orders(None).await?;

    // Filter positions by symbol if specified
    let positions = if !symbol_filter.is_empty() {
        all_positions
            .into_iter()
            .filter(|p| {
                symbol_filter
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&p.symbol))
            })
            .collect()
    } else {
        all_positions
    };

    // Filter orders by symbol if specified
    let orders = if !symbol_filter.is_empty() {
        all_orders
            .into_iter()
            .filter(|o| {
                symbol_filter
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&o.symbol))
            })
            .collect()
    } else {
        all_orders
    };

    // Fetch market data for tracked symbols
    let mut market = Vec::new();
    for symbol in &symbol_list {
        if let Ok(ticker) = client.get_symbol_market(symbol).await {
            market.push(ticker);
        }
    }

    // Create dashboard snapshot
    let snapshot = DashboardSnapshot {
        timestamp: chrono::Utc::now().to_rfc3339(),
        account,
        positions,
        orders,
        market,
    };

    match output_format {
        OutputFormat::Table => {
            println!("=== Dashboard Snapshot ===");
            println!("Timestamp: {}", snapshot.timestamp);
            println!();

            // Format account/balance as table (single row)
            if let Some(ref balance) = snapshot.account {
                println!("--- Account ---");
                println!("{}", output::format_item(balance));
                println!();
            }

            // Format positions as table
            if !snapshot.positions.is_empty() {
                println!("--- Positions ({}) ---", snapshot.positions.len());
                println!("{}", output::format_table(snapshot.positions));
                println!();
            }

            // Format orders as table
            if !snapshot.orders.is_empty() {
                println!("--- Open Orders ({}) ---", snapshot.orders.len());
                for order in &snapshot.orders {
                    println!(
                        "  {} {} {:?} {:?} @ {}",
                        order.id, order.symbol, order.side, order.order_type, order.price
                    );
                }
                println!();
            }

            // Format market data as table
            if !snapshot.market.is_empty() {
                println!("--- Market Data ({}) ---", snapshot.market.len());
                println!("{}", output::format_table(snapshot.market));
            }

            if verbose {
                println!();
                println!("--- Verbose Details ---");
                if let Some(ref balance) = snapshot.account {
                    println!("  Cross Margin: {}", balance.cross_margin);
                    println!("  Cross UPNL: {}", balance.cross_upnl);
                    println!("  PnL 24h: {}", balance.pnl_24h);
                }
            }
        }
        OutputFormat::Json => {
            println!("{}", output::format_json(&snapshot)?);
        }
        OutputFormat::Csv => {
            // For CSV, output positions as they're the most important
            if !snapshot.positions.is_empty() {
                println!("{}", output::format_csv(&snapshot.positions)?);
            } else {
                println!("No positions to display");
            }
        }
        OutputFormat::Quiet => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_time_hours() {
        let now = chrono::Utc::now().timestamp();
        let result = parse_time_string("1h", false).unwrap();
        assert!(result < now);
        assert!(result >= now - 3600);
    }

    #[test]
    fn test_parse_relative_time_days() {
        let now = chrono::Utc::now().timestamp();
        let result = parse_time_string("1d", false).unwrap();
        assert!(result < now);
        assert!(result >= now - 86400);
    }

    #[test]
    fn test_parse_iso_date() {
        let result = parse_time_string("2024-01-01", true).unwrap();
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_unix_timestamp() {
        let result = parse_time_string("1704067200", true).unwrap();
        assert_eq!(result, 1704067200);
    }

    #[test]
    fn test_parse_invalid_time() {
        assert!(parse_time_string("invalid", true).is_err());
        assert!(parse_time_string("", true).is_err());
    }

    #[test]
    fn test_parse_time_edge_cases() {
        let now = chrono::Utc::now().timestamp();

        // 测试 0 秒（边界值）
        let result = parse_time_string("0s", false).unwrap();
        assert!(result <= now && result >= now - 10); // 允许 10 秒误差

        // 测试大数字天数
        let result = parse_time_string("999d", false).unwrap();
        assert!(result < now);
        assert!(result < now - 86300000); // 999 天大约是这么多秒

        // 测试分钟
        let result = parse_time_string("30m", false).unwrap();
        assert!(result < now);
        assert!(result >= now - 1800);
    }
}
