//! Command implementations

use crate::cli::*;
use crate::config::Config;
use crate::output;
use anyhow::Result;
use futures::future::join_all;
use standx_sdk::auth::Credentials;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::error::Error as StandxError;
use standx_sdk::models::{
    BlockTrade, DashboardSnapshot, OrderSide, OrderType, PortfolioSnapshot, TimeInForce, Trade,
};
use standx_sdk::websocket::{StandXWebSocket, WsMessage};
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
                "ALO" => TimeInForce::Alo,
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

                    let config = standx_sdk::models::PositionConfig {
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
            ws.subscribe_with_interval("kline", Some(&symbol), Some(&interval))
                .await?;
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

/// Handle block trade commands
pub async fn handle_block(command: BlockCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        BlockCommands::List {
            symbol,
            limit,
            status,
        } => {
            let is_completed = match status.as_str() {
                "completed" => Some(true),
                "pending" => Some(false),
                _ => None,
            };

            let trades = client
                .get_block_trades(symbol.as_deref(), limit, is_completed)
                .await?;

            if trades.is_empty() {
                println!("No block trades found");
                return Ok(());
            }

            match output_format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&trades)?);
                }
                _ => {
                    println!("{}", format_block_trades_table(&trades));
                }
            }
        }
        BlockCommands::Watch { symbol, interval } => {
            println!(
                "Watching block trades{} (Ctrl+C to exit)\n",
                symbol
                    .as_ref()
                    .map(|s| format!(" for {}", s))
                    .unwrap_or_default()
            );

            let mut last_id: Option<i64> = None;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(interval)) => {
                        match client.get_block_trades(symbol.as_deref(), 20, None).await {
                            Ok(trades) => {
                                for trade in &trades {
                                    if last_id.map(|id| trade.id > id).unwrap_or(true) {
                                        print_block_trade_line(trade);
                                        last_id = Some(trade.id).max(last_id);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error fetching block trades: {}", e);
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        println!("\nExiting block trade watch");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_block_trades_table(trades: &[BlockTrade]) -> String {
    if trades.is_empty() {
        return "No block trades".to_string();
    }

    let header = format!(
        "{:>6} {:<12} {:>6} {:>14} {:>10} {:<12} {}",
        "ID", "SYMBOL", "SIDE", "PRICE", "QTY", "STATUS", "TIME"
    );
    let separator = "-".repeat(header.len());

    let rows: Vec<String> = trades
        .iter()
        .map(|t| {
            let time = chrono::DateTime::from_timestamp(t.expire_time, 0)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| t.expire_time.to_string());
            format!(
                "{:>6} {:<12} {:>6} {:>14} {:>10} {:<12} {}",
                t.id,
                t.symbol,
                t.side.to_uppercase(),
                t.price,
                t.qty,
                t.block_status,
                time
            )
        })
        .collect();

    format!("{}\n{}\n{}", header, separator, rows.join("\n"))
}

fn print_block_trade_line(trade: &BlockTrade) {
    let time = chrono::DateTime::from_timestamp(trade.expire_time, 0)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| trade.expire_time.to_string());

    println!(
        "🟩 Block | {} | {} {} @ {} | Qty: {} | {}",
        trade.id,
        trade.side.to_uppercase(),
        trade.symbol,
        trade.price,
        trade.qty,
        time
    );
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

// ============================================================================
// Maker bot (SIP-5A community maker yield)
// ============================================================================

/// Env var gating live order placement. The live path ships code-complete but
/// locked until it has been supervised-tested against production.
const LIVE_MAKER_ENV: &str = "STANDX_ENABLE_LIVE_MAKER";

/// Why the maker loop stopped.
enum MakerExit {
    CtrlC,
    /// Too many consecutive API errors — fail safe, not open.
    FailSafe(String),
}

/// Pending place awaiting order-id adoption (live mode): create_order only
/// returns a request id, so new open orders are matched back to recent
/// places by (side, price, qty) on the next cycle.
struct PendingPlace {
    side: OrderSide,
    price: f64,
    qty: f64,
    level: u32,
    ref_mark: f64,
    cycle: u64,
}

/// Handle maker commands
pub async fn handle_maker(
    command: MakerCommands,
    output_format: OutputFormat,
    verbose: bool,
) -> Result<()> {
    match command {
        MakerCommands::Run {
            symbol,
            spread_bps,
            band_bps,
            size,
            levels,
            level_step_bps,
            refresh_bps,
            interval,
            max_position,
            max_divergence_bps,
            no_ws,
            live,
        } => {
            run_maker(
                symbol,
                MakerRunArgs {
                    spread_bps,
                    band_bps,
                    size,
                    levels,
                    level_step_bps,
                    refresh_bps,
                    interval,
                    max_position,
                    max_divergence_bps,
                    no_ws,
                    live,
                    verbose,
                },
                output_format,
            )
            .await
        }
    }
}

struct MakerRunArgs {
    spread_bps: f64,
    band_bps: f64,
    size: f64,
    levels: u32,
    level_step_bps: f64,
    refresh_bps: f64,
    interval: u64,
    max_position: f64,
    max_divergence_bps: f64,
    no_ws: bool,
    live: bool,
    verbose: bool,
}

/// Latest market data from the WebSocket feed. Values are pre-parsed on
/// receipt so cycle reads are lock-and-go.
#[derive(Default)]
struct FeedState {
    mark: Option<f64>,
    mark_at: Option<std::time::Instant>,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    book_at: Option<std::time::Instant>,
}

/// WS cache entries older than this fall back to REST for the cycle. REST
/// polling refreshed data once per interval, so 5s keeps freshness at least
/// as good as the old behavior while tolerating slow feed ticks.
const WS_STALE_AFTER: Duration = Duration::from_secs(5);

/// Spawn the resident market-feed task: one public WS connection carrying
/// `price` + `depth_book`, written into a shared cache. The outer loop wraps
/// the SDK's internal 5-attempt reconnect — when the stream ends (attempts
/// exhausted or clean close), it rebuilds the connection from scratch, since
/// subscriptions only take effect when registered before `connect()`.
fn spawn_market_feed(
    symbol: String,
    verbose: bool,
) -> (
    Arc<RwLock<FeedState>>,
    watch::Receiver<u64>,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(RwLock::new(FeedState::default()));
    let (tx, rx) = watch::channel(0u64);
    let state_task = state.clone();

    let handle = tokio::spawn(async move {
        let mut seq = 0u64;
        loop {
            let ws = match StandXWebSocket::without_auth_with_verbose(verbose) {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("⚠️  market feed setup failed: {e}; retrying in 10s");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            let _ = ws.subscribe("price", Some(&symbol)).await;
            let _ = ws.subscribe("depth_book", Some(&symbol)).await;
            let mut events = match ws.connect().await {
                Ok(rx) => rx,
                Err(e) => {
                    eprintln!("⚠️  market feed connect failed: {e}; retrying in 10s");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            while let Some(msg) = events.recv().await {
                let now = std::time::Instant::now();
                match msg {
                    WsMessage::Price(p) if p.symbol.eq_ignore_ascii_case(&symbol) => {
                        if let Ok(mark) = p.mark_price.parse::<f64>() {
                            let mut s = state_task.write().await;
                            s.mark = Some(mark);
                            s.mark_at = Some(now);
                        }
                    }
                    WsMessage::Depth(d) if d.symbol.eq_ignore_ascii_case(&symbol) => {
                        let mut s = state_task.write().await;
                        s.best_bid = d.best_bid().and_then(|v| v.parse().ok());
                        s.best_ask = d.best_ask().and_then(|v| v.parse().ok());
                        s.book_at = Some(now);
                    }
                    _ => continue,
                }
                seq += 1;
                let _ = tx.send(seq);
            }
            // Stream ended: SDK reconnects exhausted or server closed.
            eprintln!("⚠️  market feed stream ended; rebuilding connection in 10s");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    (state, rx, handle)
}

/// One market snapshot: WS cache when fresh, REST fallback otherwise
/// (startup warm-up, feed outage, or --no-ws).
async fn market_snapshot(
    client: &StandXClient,
    symbol: &str,
    feed: Option<&Arc<RwLock<FeedState>>>,
) -> Result<(f64, Option<f64>, Option<f64>, &'static str)> {
    if let Some(feed) = feed {
        let s = feed.read().await;
        let fresh =
            |at: Option<std::time::Instant>| at.is_some_and(|t| t.elapsed() < WS_STALE_AFTER);
        if fresh(s.mark_at) && fresh(s.book_at) {
            if let Some(mark) = s.mark {
                return Ok((mark, s.best_bid, s.best_ask, "ws"));
            }
        }
    }

    let (price, depth) = tokio::join!(
        client.get_symbol_price(symbol),
        client.get_depth(symbol, Some(5))
    );
    let price = price?;
    let depth = depth?;
    let mark: f64 = price
        .mark_price
        .parse()
        .map_err(|_| anyhow::anyhow!("unparseable mark price: {}", price.mark_price))?;
    let best_bid: Option<f64> = depth.best_bid().and_then(|s| s.parse().ok());
    let best_ask: Option<f64> = depth.best_ask().and_then(|s| s.parse().ok());
    Ok((mark, best_bid, best_ask, "rest"))
}

async fn run_maker(symbol: String, args: MakerRunArgs, output_format: OutputFormat) -> Result<()> {
    use standx_sdk::maker::{self, MakerConfig, RestingQuote};

    let client = StandXClient::new()?;

    // ---- Startup: symbol metadata + invariants (fail fast) ----
    let infos = client.get_symbol_info().await?;
    let info = infos
        .iter()
        .find(|i| i.symbol.eq_ignore_ascii_case(&symbol))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown symbol '{}'. Available: {}",
                symbol,
                infos
                    .iter()
                    .map(|i| i.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    if info.status != "trading" {
        return Err(anyhow::anyhow!(
            "Symbol {} is not trading (status: {})",
            info.symbol,
            info.status
        ));
    }
    let symbol = info.symbol.clone(); // canonical casing

    let min_order_qty: f64 = info.min_order_qty.parse().unwrap_or(0.0);
    let cfg = MakerConfig {
        spread_bps: args.spread_bps,
        band_bps: args.band_bps,
        level_step_bps: args.level_step_bps,
        refresh_bps: args.refresh_bps,
        levels: args.levels.max(1),
        size: args.size,
        max_position: args.max_position,
        price_decimals: info.price_tick_decimals,
        qty_decimals: info.qty_tick_decimals,
        min_order_qty,
    };

    if cfg.spread_bps <= 0.0 {
        return Err(anyhow::anyhow!("--spread-bps must be > 0"));
    }
    if cfg.band_bps <= cfg.spread_bps {
        return Err(anyhow::anyhow!(
            "--band-bps ({}) must be greater than --spread-bps ({}): quotes clamped to the band edge would sit exactly at the boundary",
            cfg.band_bps,
            cfg.spread_bps
        ));
    }
    let rounded_size = maker::round_to_decimals(cfg.size, cfg.qty_decimals);
    if rounded_size < cfg.min_order_qty || rounded_size <= 0.0 {
        return Err(anyhow::anyhow!(
            "--size {} (rounded to {} at {} decimals) is below min order qty {} for {}",
            cfg.size,
            rounded_size,
            cfg.qty_decimals,
            cfg.min_order_qty,
            symbol
        ));
    }
    if cfg.refresh_bps >= cfg.spread_bps {
        eprintln!(
            "⚠️  --refresh-bps ({}) >= --spread-bps ({}): quotes will be held through large drifts",
            cfg.refresh_bps, cfg.spread_bps
        );
    }
    if cfg.levels > 1
        && cfg.spread_bps + (cfg.levels - 1) as f64 * cfg.level_step_bps >= cfg.band_bps
    {
        eprintln!("⚠️  outer quote levels exceed the band and will be clamped/collapsed");
    }

    // ---- Live gating & clean start ----
    if args.live {
        if std::env::var(LIVE_MAKER_ENV).ok().as_deref() != Some("1") {
            return Err(anyhow::anyhow!(
                "live mode not yet enabled: it has not been supervised-tested against production. Set {}=1 to unlock (at your own risk).",
                LIVE_MAKER_ENV
            ));
        }
        let creds = Credentials::load()?;
        if creds.is_expired() {
            return Err(anyhow::anyhow!(
                "Credentials expired. Run 'standx auth login' first."
            ));
        }
        if creds.private_key.is_empty() {
            return Err(anyhow::anyhow!(
                "Live mode requires a private key for order signing. Run 'standx auth login' with --private-key."
            ));
        }
        // Start from a clean book so reconciliation isn't confused by
        // leftovers from a previous run. The bot owns ALL orders on this
        // symbol while running.
        client.cancel_all_orders(&symbol).await?;
    }

    let mode = if args.live { "LIVE" } else { "PAPER" };
    if output_format == OutputFormat::Table {
        println!("┌──────────────────────────────────────────────────────────┐");
        println!("│ standx maker — {} mode on {}", mode, symbol);
        println!(
            "│ spread {}bps | band {}bps | refresh {}bps | {} level(s)",
            cfg.spread_bps, cfg.band_bps, cfg.refresh_bps, cfg.levels
        );
        println!(
            "│ size {} | max-position {} | interval {}s",
            cfg.size, cfg.max_position, args.interval
        );
        println!(
            "│ ticks: price {}dp, qty {}dp | min qty {}",
            cfg.price_decimals, cfg.qty_decimals, cfg.min_order_qty
        );
        if !args.live {
            println!("│ paper mode: no orders are placed; fills are NOT simulated");
            println!("│ (position stays 0). Add --live for real quoting.");
        } else {
            println!(
                "│ ⚠️  LIVE: the bot manages ALL orders on {} — manual",
                symbol
            );
            println!("│ orders on this symbol will be cancelled as stale.");
        }
        if args.no_ws {
            println!("│ feed: REST polling (--no-ws)");
        } else {
            println!(
                "│ feed: websocket (REST fallback) | divergence guard {}bps",
                args.max_divergence_bps
            );
        }
        println!("│ Ctrl+C to stop (cancels all resting orders on exit)");
        println!("└──────────────────────────────────────────────────────────┘");
    }

    // ---- Market feed (WS primary, REST fallback) ----
    let (feed, mut updates, feed_handle) = if args.no_ws {
        (None, None, None)
    } else {
        let (state, rx, handle) = spawn_market_feed(symbol.clone(), args.verbose);
        (Some(state), Some(rx), Some(handle))
    };

    // ---- Loop state ----
    let mut cycle: u64 = 0;
    let mut resting: Vec<RestingQuote> = Vec::new(); // paper-mode book
    let mut adopted: HashMap<String, (u32, f64, u64)> = HashMap::new(); // id -> (level, ref_mark, cycle)
    let mut pending: Vec<PendingPlace> = Vec::new();
    let mut consecutive_errors: u32 = 0;
    let mut total_places: u64 = 0;
    let mut total_cancels: u64 = 0;
    let mut total_holds: u64 = 0;
    let mut last_mark: Option<f64> = None;
    let mut last_src: Option<&'static str> = None;

    let exit = 'main: loop {
        // Work phase raced against Ctrl+C so a slow API call can be
        // interrupted (mirrors run_watch_loop).
        let work = async {
            let (mark, best_bid, best_ask, src) =
                market_snapshot(&client, &symbol, feed.as_ref()).await?;
            let (places, cancels, holds) = maker_cycle(
                &client,
                &symbol,
                &cfg,
                args.live,
                cycle,
                mark,
                best_bid,
                best_ask,
                args.max_divergence_bps,
                &mut resting,
                &mut adopted,
                &mut pending,
                output_format,
            )
            .await?;
            Ok::<_, anyhow::Error>((places, cancels, holds, mark, src))
        };
        let cycle_result = tokio::select! {
            _ = signal::ctrl_c() => break MakerExit::CtrlC,
            result = work => result,
        };

        match cycle_result {
            Ok((places, cancels, holds, mark, src)) => {
                consecutive_errors = 0;
                total_places += places;
                total_cancels += cancels;
                total_holds += holds;
                last_mark = Some(mark);
                if !args.no_ws && last_src != Some(src) {
                    match src {
                        "ws" => eprintln!("✅ market feed: websocket live"),
                        _ => eprintln!(
                            "⚠️  market feed: REST fallback (websocket warming up or stale)"
                        ),
                    }
                    last_src = Some(src);
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("⚠️  maker cycle failed ({}/3): {}", consecutive_errors, e);
                if consecutive_errors >= 3 {
                    break MakerExit::FailSafe(e.to_string());
                }
            }
        }

        cycle += 1;

        // Sleep until the next cycle, but wake early when the cached mark
        // has already drifted beyond refresh_bps — the quotes would be
        // re-quoted anyway, so reacting now shrinks the pick-off window
        // without adding flicker. min-gap of 1s bounds the API rate.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(args.interval);
        let min_gap = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let update = async {
                match updates.as_mut() {
                    Some(rx) => rx.changed().await.is_ok(),
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = signal::ctrl_c() => break 'main MakerExit::CtrlC,
                _ = tokio::time::sleep_until(deadline) => break,
                ok = update => {
                    if !ok {
                        // Feed task gone: fall back to plain interval waits.
                        updates = None;
                        continue;
                    }
                    if tokio::time::Instant::now() < min_gap {
                        continue;
                    }
                    let (Some(feed), Some(prev)) = (feed.as_ref(), last_mark) else {
                        continue;
                    };
                    let drifted = {
                        let s = feed.read().await;
                        s.mark
                            .is_some_and(|m| maker::bps_diff(m, prev) > cfg.refresh_bps)
                    };
                    if drifted {
                        break; // early re-quote cycle
                    }
                }
            }
        }
    };

    // ---- Cleanup on ALL exit paths ----
    if let Some(handle) = feed_handle {
        handle.abort();
    }
    if output_format == OutputFormat::Table {
        println!(
            "\n👋 Stopping maker (ran {} cycles: {} places, {} cancels, {} holds)",
            cycle, total_places, total_cancels, total_holds
        );
    }
    if args.live {
        cancel_all_with_retry(&client, &symbol, 3).await?;
    }

    match exit {
        MakerExit::CtrlC => Ok(()),
        MakerExit::FailSafe(e) => Err(anyhow::anyhow!(
            "maker stopped after 3 consecutive errors (fail-safe): {}",
            e
        )),
    }
}

/// One reconcile cycle over an already-acquired market snapshot.
/// Returns (places, cancels, holds) counts.
#[allow(clippy::too_many_arguments)]
async fn maker_cycle(
    client: &StandXClient,
    symbol: &str,
    cfg: &standx_sdk::maker::MakerConfig,
    live: bool,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
    resting: &mut Vec<standx_sdk::maker::RestingQuote>,
    adopted: &mut HashMap<String, (u32, f64, u64)>,
    pending: &mut Vec<PendingPlace>,
    output_format: OutputFormat,
) -> Result<(u64, u64, u64)> {
    use standx_sdk::maker::{
        compute_desired_quotes, format_decimals, mark_mid_divergence_bps, reconcile, Action,
        RestingQuote,
    };

    // 1. Sanity guard: when mark and the book mid disagree, at least one
    //    data source is wrong (stale feed, bad print, dislocated book).
    //    Acting on it is unsafe in every direction, so do nothing this
    //    cycle: resting quotes stay untouched. Not a fail-safe error.
    if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
        let divergence = mark_mid_divergence_bps(mark, bid, ask);
        if divergence > max_divergence_bps {
            let live_str = if live { "live" } else { "paper" };
            match output_format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "cycle": cycle, "mode": live_str, "symbol": symbol,
                            "action": "skip", "reason": "mark_mid_divergence",
                            "mark": format_decimals(mark, cfg.price_decimals),
                            "divergence_bps": (divergence * 100.0).round() / 100.0,
                            "max_divergence_bps": max_divergence_bps,
                        })
                    );
                }
                _ => {
                    eprintln!(
                        "⚠️  #{} mark/mid divergence {:.1}bps > {}bps — skipping cycle (no actions)",
                        cycle, divergence, max_divergence_bps
                    );
                }
            }
            return Ok((0, 0, 0));
        }
    }

    if live && (best_bid.is_none() || best_ask.is_none()) {
        // Fail-safe: without a touch we cannot guarantee no-cross pricing.
        eprintln!("⚠️  empty order book on {}; skipping this cycle", symbol);
        return Ok((0, 0, 0));
    }

    // 2. Rebuild resting + position from the exchange (live) or keep the
    //    simulated book (paper).
    let position: f64;
    if live {
        let (orders, positions) = tokio::join!(
            client.get_open_orders(Some(symbol)),
            client.get_positions(Some(symbol))
        );
        let orders = orders?;
        let positions = positions?;

        position = positions
            .iter()
            .filter(|p| p.symbol.eq_ignore_ascii_case(symbol))
            .map(|p| {
                let qty: f64 = p.qty.parse().unwrap_or(0.0);
                match p.side {
                    Some(OrderSide::Sell) => -qty,
                    _ => qty,
                }
            })
            .sum();

        let tick = cfg.price_tick();
        *resting = orders
            .into_iter()
            .map(|o| {
                let price: f64 = o.price.parse().unwrap_or(0.0);
                let qty: f64 = o.qty.parse().unwrap_or(0.0);
                let (level, ref_mark, placed_at_cycle) = match adopted.get(&o.id) {
                    Some(&meta) => meta,
                    None => {
                        // Try to adopt from a recent place by (side, price, qty).
                        let matched = pending.iter().position(|p| {
                            p.side == o.side
                                && (p.price - price).abs() < tick / 2.0
                                && (p.qty - qty).abs() < f64::EPSILON.max(qty * 1e-6)
                        });
                        let meta = match matched {
                            Some(idx) => {
                                let p = pending.remove(idx);
                                (p.level, p.ref_mark, p.cycle)
                            }
                            // Unknown order (manual or unmatched): sentinel
                            // level so reconcile cancels it as stale — the
                            // bot owns all orders on this symbol.
                            None => (u32::MAX, mark, cycle),
                        };
                        adopted.insert(o.id.clone(), meta);
                        meta
                    }
                };
                RestingQuote {
                    order_id: Some(o.id),
                    side: o.side,
                    level,
                    price,
                    qty,
                    ref_mark,
                    placed_at_cycle,
                }
            })
            .collect();
        // Places older than 2 cycles never showed up as open orders —
        // likely rejected (e.g. ALO would-cross) or instantly filled.
        pending.retain(|p| cycle.saturating_sub(p.cycle) <= 2);
        adopted.retain(|id, _| resting.iter().any(|r| r.order_id.as_deref() == Some(id)));
    } else {
        position = 0.0; // fills are not simulated in paper mode
    }

    // 3. Decide.
    let desired = compute_desired_quotes(cfg, mark, best_bid, best_ask, position);
    let actions = reconcile(cfg, mark, best_bid, best_ask, &desired, resting, cycle);

    // 4. Execute.
    let mut places: u64 = 0;
    let mut cancels: u64 = 0;
    let mut holds: u64 = 0;
    for action in &actions {
        match action {
            Action::Cancel {
                order_id,
                side,
                level,
                ..
            } => {
                cancels += 1;
                if live {
                    if let Some(id) = order_id {
                        client.cancel_order(symbol, id).await?;
                        adopted.remove(id);
                    }
                } else {
                    resting.retain(|r| !(r.side == *side && r.level == *level));
                }
            }
            Action::Place(q) => {
                places += 1;
                if live {
                    client
                        .create_order(CreateOrderParams {
                            symbol: symbol.to_string(),
                            side: q.side,
                            order_type: OrderType::Limit,
                            quantity: format_decimals(q.qty, cfg.qty_decimals),
                            price: Some(format_decimals(q.price, cfg.price_decimals)),
                            // Post-only: reject instead of taking if the
                            // price would cross by arrival time.
                            time_in_force: Some(TimeInForce::Alo),
                            reduce_only: false,
                            stop_price: None,
                            sl_price: None,
                            tp_price: None,
                        })
                        .await?;
                    pending.push(PendingPlace {
                        side: q.side,
                        price: q.price,
                        qty: q.qty,
                        level: q.level,
                        ref_mark: mark,
                        cycle,
                    });
                } else {
                    resting.push(RestingQuote {
                        order_id: None,
                        side: q.side,
                        level: q.level,
                        price: q.price,
                        qty: q.qty,
                        ref_mark: mark,
                        placed_at_cycle: cycle,
                    });
                }
            }
            Action::Hold { .. } => holds += 1,
        }
    }

    // 5. Emit.
    emit_maker_cycle(
        output_format,
        live,
        symbol,
        cycle,
        mark,
        best_bid,
        best_ask,
        position,
        &actions,
        cfg,
    );

    Ok((places, cancels, holds))
}

/// Cancel-all with retries; verifies the book is actually clean afterwards.
async fn cancel_all_with_retry(client: &StandXClient, symbol: &str, attempts: u32) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        match client.cancel_all_orders(symbol).await {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                eprintln!(
                    "⚠️  cancel-all attempt {}/{} failed: {}",
                    attempt, attempts, e
                );
                last_err = Some(e.into());
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    // Verify: a failed cancel leaves live orders unattended.
    match client.get_open_orders(Some(symbol)).await {
        Ok(orders) if orders.is_empty() => {
            println!("✅ All {} orders cancelled", symbol);
            Ok(())
        }
        Ok(orders) => {
            let ids: Vec<_> = orders.iter().map(|o| o.id.as_str()).collect();
            Err(anyhow::anyhow!(
                "⚠️  RESIDUAL ORDERS on {} after cancel-all: [{}] — cancel manually with 'standx order cancel-all {}'",
                symbol,
                ids.join(", "),
                symbol
            ))
        }
        Err(e) => match last_err {
            Some(cancel_err) => Err(anyhow::anyhow!(
                "cancel-all failed ({}) and verification failed ({}) — check open orders manually",
                cancel_err,
                e
            )),
            None => Err(anyhow::anyhow!(
                "cancel-all succeeded but verification failed ({}) — check open orders manually",
                e
            )),
        },
    }
}

/// Per-cycle output: one human line + indented actions, or JSON lines.
#[allow(clippy::too_many_arguments)]
fn emit_maker_cycle(
    output_format: OutputFormat,
    live: bool,
    symbol: &str,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    position: f64,
    actions: &[standx_sdk::maker::Action],
    cfg: &standx_sdk::maker::MakerConfig,
) {
    use standx_sdk::maker::{format_decimals, Action};

    let mode = if live { "live" } else { "paper" };
    let counts = actions.iter().fold((0, 0, 0), |mut acc, a| {
        match a {
            Action::Place(_) => acc.1 += 1,
            Action::Cancel { .. } => acc.2 += 1,
            Action::Hold { .. } => acc.0 += 1,
        }
        acc
    });
    let (holds, places, cancels) = counts;

    match output_format {
        OutputFormat::Json => {
            let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            for a in actions {
                let obj = match a {
                    Action::Place(q) => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "place", "side": q.side, "level": q.level,
                        "price": format_decimals(q.price, cfg.price_decimals),
                        "qty": format_decimals(q.qty, cfg.qty_decimals),
                    }),
                    Action::Cancel {
                        order_id,
                        side,
                        level,
                        price,
                        reason,
                    } => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "cancel", "side": side, "level": level,
                        "price": format_decimals(*price, cfg.price_decimals),
                        "reason": reason.as_str(), "order_id": order_id,
                    }),
                    Action::Hold {
                        side,
                        level,
                        price,
                        age_cycles,
                        drift_bps,
                    } => serde_json::json!({
                        "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                        "mark": format_decimals(mark, cfg.price_decimals),
                        "action": "hold", "side": side, "level": level,
                        "price": format_decimals(*price, cfg.price_decimals),
                        "age_cycles": age_cycles,
                        "drift_bps": (drift_bps * 100.0).round() / 100.0,
                    }),
                };
                println!("{}", obj);
            }
            println!(
                "{}",
                serde_json::json!({
                    "ts": ts, "cycle": cycle, "mode": mode, "symbol": symbol,
                    "action": "cycle_summary",
                    "mark": format_decimals(mark, cfg.price_decimals),
                    "best_bid": best_bid, "best_ask": best_ask,
                    "position": position,
                    "holds": holds, "places": places, "cancels": cancels,
                })
            );
        }
        OutputFormat::Quiet => {
            // Only mutations and their reasons.
            for a in actions {
                match a {
                    Action::Place(q) => println!(
                        "place {} L{} @ {}",
                        side_str(q.side),
                        q.level,
                        format_decimals(q.price, cfg.price_decimals)
                    ),
                    Action::Cancel {
                        side,
                        level,
                        price,
                        reason,
                        ..
                    } => println!(
                        "cancel {} L{} @ {} ({})",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        reason.as_str()
                    ),
                    Action::Hold { .. } => {}
                }
            }
        }
        _ => {
            let now = chrono::Local::now().format("%H:%M:%S");
            println!(
                "[{}] #{} mark={} bid={} ask={} pos={} | hold={} place={} cancel={}",
                now,
                cycle,
                format_decimals(mark, cfg.price_decimals),
                best_bid
                    .map(|b| format_decimals(b, cfg.price_decimals))
                    .unwrap_or_else(|| "-".into()),
                best_ask
                    .map(|a| format_decimals(a, cfg.price_decimals))
                    .unwrap_or_else(|| "-".into()),
                position,
                holds,
                places,
                cancels
            );
            for a in actions {
                match a {
                    Action::Place(q) => println!(
                        "    PLACE  {} L{} @ {} x {}",
                        side_str(q.side),
                        q.level,
                        format_decimals(q.price, cfg.price_decimals),
                        format_decimals(q.qty, cfg.qty_decimals)
                    ),
                    Action::Cancel {
                        side,
                        level,
                        price,
                        reason,
                        ..
                    } => println!(
                        "    CANCEL {} L{} @ {} ({})",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        reason.as_str()
                    ),
                    Action::Hold {
                        side,
                        level,
                        price,
                        age_cycles,
                        drift_bps,
                    } => println!(
                        "    HOLD   {} L{} @ {} (age {} cycles, drift {:.1}bps)",
                        side_str(*side),
                        level,
                        format_decimals(*price, cfg.price_decimals),
                        age_cycles,
                        drift_bps
                    ),
                }
            }
        }
    }
}

fn side_str(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "buy ",
        OrderSide::Sell => "sell",
    }
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
