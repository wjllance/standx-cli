//! Command implementations

use crate::cli::*;
use anyhow::Result;
use futures::future::join_all;
use standx_cli::auth::Credentials;
use standx_cli::client::order::CreateOrderParams;
use standx_cli::client::StandXClient;
use standx_cli::config::Config;
use standx_cli::error::Error as StandxError;
use standx_cli::models::{
    DashboardSnapshot, OrderSide, OrderType, PortfolioSnapshot, TimeInForce, Trade,
};
use standx_cli::output;
use standx_cli::websocket::{StandXWebSocket, WsMessage};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{watch, RwLock};

/// Portfolio command for direct execution (without subcommands)
#[derive(Debug)]
pub enum PortfolioCommand {
    Snapshot { _verbose: bool, watch: Option<u64> },
}

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
            let mut positions = client.get_positions(symbol.as_deref()).await?;

            // Filter out positions with qty = 0
            positions.retain(|p| p.qty.parse::<f64>().unwrap_or(0.0) > 0.0);

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
            // Check if stdin is a TTY
            let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());

            // Get token - use provided token or prompt if in TTY and no token provided
            let token = if let Some(t) = token {
                // Token provided via -t flag
                t
            } else if let Some(file) = token_file {
                // Token provided via file
                std::fs::read_to_string(file)?.trim().to_string()
            } else if is_tty || interactive {
                // Interactive prompt
                println!("Enter JWT Token:");
                rpassword::prompt_password("Token: ")?.trim().to_string()
            } else {
                anyhow::bail!(
                    "Token required in non-interactive mode. Provide token via -t flag or -t FILE"
                );
            };

            // Get private key - skip if not provided and not in TTY
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
            } else if is_tty {
                // Interactive prompt - only if TTY available
                println!("\nEnter Ed25519 Private Key (Base58) - optional, press Enter to skip:");
                let key = rpassword::prompt_password("Private Key: ")?
                    .trim()
                    .to_string();
                if key.is_empty() {
                    None
                } else {
                    Some(key)
                }
            } else {
                // Non-TTY: skip private key (optional)
                None
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
                        // Format timestamp to readable time
                        let time_str = match kline.time.parse::<i64>() {
                            Ok(ts) => chrono::DateTime::from_timestamp(ts, 0)
                                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| kline.time.clone()),
                            Err(_) => kline.time.clone(),
                        };
                        println!(
                            "  {}: O:{} H:{} L:{} C:{} V:{}",
                            time_str, kline.open, kline.high, kline.low, kline.close, kline.volume
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
        StreamCommands::Kline { symbol, interval } => {
            let ws = StandXWebSocket::without_auth_with_verbose(verbose)?;
            // Subscribe with interval parameter embedded in topic
            let topic = format!("{}:{}:{}", "kline", symbol, interval);
            ws.subscribe_with_interval("kline", Some(&symbol), Some(&interval)).await?;
            let mut rx = ws.connect().await?;

            println!("Streaming kline for {} [{}]", symbol, interval);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                if let WsMessage::Kline(data) = msg {
                    // Convert timestamp to readable time
                    let time_str = chrono::DateTime::from_timestamp_millis(data.time)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| data.time.to_string());
                    
                    println!(
                        "📊 Kline: {} [{}] {}\nO: {}  H: {}  L: {}  C: {}  Vol: {:.3}",
                        data.symbol.unwrap_or_default(),
                        data.interval.unwrap_or_default(),
                        time_str,
                        data.open,
                        data.high,
                        data.low,
                        data.close,
                        data.volume
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

fn is_auth_error(error: &StandxError) -> bool {
    matches!(
        error,
        StandxError::AuthRequired { .. }
            | StandxError::TokenExpired { .. }
            | StandxError::InvalidCredentials { .. }
            | StandxError::Api { code: 401, .. }
    )
}

async fn run_watch_loop<F, Fut>(
    watch: Option<u64>,
    mut render_once: F,
    error_prefix: &str,
    mut update_rx: Option<watch::Receiver<u64>>,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    if let Some(interval_secs) = watch {
        loop {
            let render_result = tokio::select! {
                _ = signal::ctrl_c() => {
                    println!("\n👋 Stopping watch mode");
                    break;
                }
                result = render_once() => result,
            };

            match render_result {
                Ok(rendered) => {
                    // Clear only after new frame is ready, reducing flicker.
                    print!("\x1B[2J\x1B[1H");
                    print!("{}", rendered);
                }
                Err(e) => {
                    eprintln!("⚠️  {}: {}", error_prefix, e);
                }
            }

            tokio::select! {
                _ = signal::ctrl_c() => {
                    println!("\n👋 Stopping watch mode");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
                ws_updated = async {
                    if let Some(rx) = update_rx.as_mut() {
                        rx.changed().await.is_ok()
                    } else {
                        std::future::pending::<bool>().await
                    }
                } => {
                    // If sender is dropped, disable event-triggered refresh and keep interval refresh.
                    if !ws_updated {
                        update_rx = None;
                    }
                }
            }
        }
        Ok(())
    } else {
        let rendered = render_once().await?;
        print!("{}", rendered);
        Ok(())
    }
}

/// Handle dashboard commands - unified view of account, positions, orders, and market data
pub async fn handle_dashboard(
    symbols: Option<String>,
    verbose: bool,
    watch: Option<u64>,
    compact: bool,
    output_format: OutputFormat,
) -> Result<()> {
    // Build list of symbols to track
    let symbol_list: Vec<String> = if let Some(s) = symbols {
        s.split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .collect()
    } else {
        vec![]
    };
    let client = StandXClient::new()?;
    let ws_trades: Arc<RwLock<VecDeque<Trade>>> = Arc::new(RwLock::new(VecDeque::new()));
    let mut ws_trade_updates_rx: Option<watch::Receiver<u64>> = None;
    let mut ws_trades_enabled = false;

    if watch.is_some() {
        let first_symbol = if let Some(symbol) = symbol_list.first() {
            Some(symbol.clone())
        } else {
            client
                .get_symbol_info()
                .await
                .ok()
                .and_then(|symbols| symbols.into_iter().next().map(|s| s.symbol))
        };

        if let Some(first_symbol) = first_symbol {
            // Seed initial trades so first frame has data even before websocket receives updates.
            if let Ok(initial_trades) = client.get_recent_trades(&first_symbol, Some(7)).await {
                let mut buf = ws_trades.write().await;
                for trade in initial_trades {
                    buf.push_back(trade);
                }
                while buf.len() > 7 {
                    buf.pop_back();
                }
            }

            if let Ok(ws) = StandXWebSocket::without_auth() {
                if ws
                    .subscribe("public_trade", Some(&first_symbol))
                    .await
                    .is_ok()
                {
                    let (trade_updates_tx, trade_updates_rx) = watch::channel(0_u64);
                    ws_trade_updates_rx = Some(trade_updates_rx);
                    let mut update_seq: u64 = 0;
                    if let Ok(mut rx) = ws.connect().await {
                        ws_trades_enabled = true;
                        let ws_trades_clone = ws_trades.clone();
                        tokio::spawn(async move {
                            while let Some(msg) = rx.recv().await {
                                if let WsMessage::Trade(trade) = msg {
                                    let mut trades = ws_trades_clone.write().await;
                                    trades.push_front(trade);
                                    while trades.len() > 7 {
                                        trades.pop_back();
                                    }
                                    update_seq = update_seq.wrapping_add(1);
                                    let _ = trade_updates_tx.send(update_seq);
                                }
                            }
                        });
                    }
                }
            }
        }
    }

    run_watch_loop(
        watch,
        || {
            build_dashboard_output(
                &client,
                &symbol_list,
                verbose,
                output_format,
                compact,
                if ws_trades_enabled {
                    Some(ws_trades.clone())
                } else {
                    None
                },
            )
        },
        "Dashboard refresh failed",
        ws_trade_updates_rx,
    )
    .await
}

/// Build dashboard output with optional symbol filtering
async fn build_dashboard_output(
    client: &StandXClient,
    symbol_filter: &[String],
    _verbose: bool,
    output_format: OutputFormat,
    compact: bool,
    ws_trades: Option<Arc<RwLock<VecDeque<Trade>>>>,
) -> Result<String> {
    // Check if filtering by symbols
    let has_filter = !symbol_filter.is_empty();

    // Determine which symbols to track
    let symbol_list: Vec<String> = if has_filter {
        symbol_filter.to_vec()
    } else {
        // Get all available symbols from API
        client
            .get_symbol_info()
            .await?
            .into_iter()
            .map(|s| s.symbol)
            .collect()
    };

    // Fetch authenticated endpoints concurrently
    let (balance_result, positions_result, orders_result) = tokio::join!(
        client.get_balance(),
        client.get_positions(None),
        client.get_open_orders(None)
    );

    // Try to fetch authenticated data, handle auth errors gracefully
    let (account, auth_warning) = match balance_result {
        Ok(balance) => (Some(balance), None),
        Err(e) => {
            if is_auth_error(&e) {
                (
                    None,
                    Some("⚠️  Not authenticated. Run 'standx auth login' to access account data."),
                )
            } else {
                return Err(e.into());
            }
        }
    };

    let all_positions = match positions_result {
        Ok(positions) => positions,
        Err(e) if is_auth_error(&e) => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let total_realized_pnl_all_positions: f64 = all_positions
        .iter()
        .map(|p| p.realized_pnl.parse::<f64>().unwrap_or(0.0))
        .sum();
    let all_orders = match orders_result {
        Ok(orders) => orders,
        Err(e) if is_auth_error(&e) => Vec::new(),
        Err(e) => return Err(e.into()),
    };

    // Filter by symbol if specified, and filter out zero-qty positions
    let positions = if has_filter {
        all_positions
            .into_iter()
            .filter(|p| {
                p.qty.parse::<f64>().unwrap_or(0.0) > 0.0
                    && symbol_filter
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(&p.symbol))
            })
            .collect()
    } else {
        all_positions
            .into_iter()
            .filter(|p| p.qty.parse::<f64>().unwrap_or(0.0) > 0.0)
            .collect()
    };

    let orders = if has_filter {
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

    // Fetch market + kline data for tracked symbols in parallel.
    // Kline open is used as a fallback to compute 24h change when ticker field is missing.
    let now_ts = chrono::Utc::now().timestamp();
    let from_ts = now_ts - 86400;
    let (market_results, kline_results) = tokio::join!(
        join_all(
            symbol_list
                .iter()
                .map(|symbol| client.get_symbol_market(symbol))
        ),
        join_all(
            symbol_list
                .iter()
                .map(|symbol| client.get_kline(symbol, "1D", from_ts, now_ts))
        )
    );

    let mut open_prices: HashMap<String, f64> = HashMap::new();
    for (index, result) in kline_results.into_iter().enumerate() {
        if let Ok(klines) = result {
            if let Some(kline) = klines.first() {
                if let Ok(open) = kline.open.parse::<f64>() {
                    if open > 0.0 {
                        open_prices.insert(symbol_list[index].clone(), open);
                    }
                }
            }
        }
    }

    let mut market: Vec<_> = market_results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();

    for ticker in &mut market {
        if ticker.change_24h_percent.is_empty() || ticker.change_24h_percent == "0" {
            if let Some(open) = open_prices.get(&ticker.symbol) {
                if let Ok(last) = ticker.last_price.parse::<f64>() {
                    let change = ((last - open) / open) * 100.0;
                    ticker.change_24h_percent = format!("{:.2}", change);
                }
            }
        }
    }

    // Fetch recent trades + order book for first symbol.
    // In watch mode we prefer websocket-fed trades buffer to avoid polling for trades.
    let (trades, order_book) = if let Some(first_symbol) = symbol_list.first() {
        let trades = if let Some(ws_buf) = ws_trades {
            let buf = ws_buf.read().await;
            buf.iter().cloned().collect()
        } else {
            client
                .get_recent_trades(first_symbol, Some(7))
                .await
                .unwrap_or_default()
        };

        let order_book = client.get_depth(first_symbol, Some(5)).await.ok();
        (trades, order_book)
    } else {
        (Vec::new(), None)
    };

    // Create dashboard snapshot
    let snapshot = DashboardSnapshot {
        timestamp: chrono::Utc::now().to_rfc3339(),
        account,
        positions,
        total_realized_pnl: total_realized_pnl_all_positions.to_string(),
        orders,
        market,
        trades,
        order_book,
    };

    let rendered = match output_format {
        OutputFormat::Table => {
            // Use MVP format (Issue #156)
            let mut text = String::new();
            if let Some(warning) = auth_warning {
                text.push_str(warning);
                text.push_str("\n\n");
            }
            text.push_str(&output::format_dashboard_mvp(&snapshot, compact));
            text
        }
        OutputFormat::Json => format!("{}\n", output::format_json(&snapshot)?),
        OutputFormat::Csv => {
            // For CSV, output positions as they're the most important
            if !snapshot.positions.is_empty() {
                format!("{}\n", output::format_csv(&snapshot.positions)?)
            } else {
                "No positions to display\n".to_string()
            }
        }
        OutputFormat::Quiet => String::new(),
    };

    Ok(rendered)
}

/// Handle portfolio commands - view portfolio summary and performance
pub async fn handle_portfolio(
    command: PortfolioCommand,
    output_format: OutputFormat,
) -> Result<()> {
    match command {
        PortfolioCommand::Snapshot { _verbose, watch } => {
            let client = StandXClient::new()?;
            run_watch_loop(
                watch,
                || build_portfolio_output(&client, _verbose, output_format),
                "Portfolio refresh failed",
                None,
            )
            .await?;
        }
    }
    Ok(())
}

/// Build portfolio output
async fn build_portfolio_output(
    client: &StandXClient,
    verbose: bool,
    output_format: OutputFormat,
) -> Result<String> {
    // Try to fetch authenticated data, handle auth errors gracefully
    let balance_result = client.get_balance().await;
    let balance = match balance_result {
        Ok(b) => Some(b),
        Err(e) => {
            if is_auth_error(&e) {
                None
            } else {
                return Err(e.into());
            }
        }
    };

    // If not authenticated, show market data only
    let positions = if balance.is_some() {
        let positions = match client.get_positions(None).await {
            Ok(positions) => positions,
            Err(e) if is_auth_error(&e) => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        // Filter out zero-qty positions
        positions
            .into_iter()
            .filter(|p| p.qty.parse::<f64>().unwrap_or(0.0) > 0.0)
            .collect()
    } else {
        Vec::new()
    };

    // Calculate total values
    let total_value_usd = balance
        .as_ref()
        .map(|b| b.equity.clone())
        .unwrap_or_default();
    let total_pnl_24h = balance
        .as_ref()
        .map(|b| b.pnl_24h.clone())
        .unwrap_or_default();
    let total_pnl_realized = balance.as_ref().map(|b| b.upnl.clone()).unwrap_or_default();

    // Create portfolio snapshot
    let snapshot = PortfolioSnapshot {
        timestamp: chrono::Utc::now().to_rfc3339(),
        total_value_usd,
        total_pnl_24h,
        total_pnl_realized,
        positions,
    };

    let rendered = match output_format {
        OutputFormat::Table => {
            let mut text = String::new();
            if balance.is_none() {
                text.push_str(
                    "⚠️  Not authenticated. Run 'standx auth login' to access account data.\n\n",
                );
            }
            text.push_str("=== Portfolio Summary ===\n");
            text.push_str(&format!("Timestamp: {}\n\n", snapshot.timestamp));

            // Account summary
            text.push_str("--- Account ---\n");
            text.push_str(&format!("  Total Value: ${}\n", snapshot.total_value_usd));
            text.push_str(&format!("  PnL 24h: ${}\n", snapshot.total_pnl_24h));
            text.push_str(&format!(
                "  Unrealized PnL: ${}\n\n",
                snapshot.total_pnl_realized
            ));

            // Positions
            if !snapshot.positions.is_empty() {
                text.push_str(&format!(
                    "--- Positions ({}) ---\n",
                    snapshot.positions.len()
                ));
                text.push_str(&format!("{}\n", output::format_table(snapshot.positions)));
            } else {
                text.push_str("--- No open positions ---\n");
            }

            if verbose {
                text.push_str("\n--- Verbose Details ---\n");
                if let Some(b) = &balance {
                    text.push_str(&format!("  Balance: ${}\n", b.balance));
                    text.push_str(&format!("  Available: ${}\n", b.cross_available));
                    text.push_str(&format!("  Equity: ${}\n", b.equity));
                    text.push_str(&format!("  Cross Margin: ${}\n", b.cross_margin));
                    text.push_str(&format!("  Cross UPNL: ${}\n", b.cross_upnl));
                    text.push_str(&format!("  Locked: ${}\n", b.locked));
                } else {
                    text.push_str("  (Not authenticated - no balance details)\n");
                }
            }
            text
        }
        OutputFormat::Json => format!("{}\n", output::format_json(&snapshot)?),
        OutputFormat::Csv => {
            if !snapshot.positions.is_empty() {
                format!("{}\n", output::format_csv(&snapshot.positions)?)
            } else {
                "No positions to display\n".to_string()
            }
        }
        OutputFormat::Quiet => String::new(),
    };

    Ok(rendered)
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
