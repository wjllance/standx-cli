//! Command implementations

use crate::cli::*;
use anyhow::Result;
use standx_cli::auth::Credentials;
use standx_cli::client::order::CreateOrderParams;
use standx_cli::client::StandXClient;
use standx_cli::config::Config;
use standx_cli::models::{OrderBook, OrderSide, OrderType, TimeInForce};
use standx_cli::output;
use standx_cli::websocket::{StandXWebSocket, WsMessage};

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
            println!("   Quantity: {}", order.quantity);
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
pub async fn handle_config(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Init => {
            let config = Config::default();
            config.save()?;
            println!("✅ Configuration initialized at {:?}", config.config_file());
        }
        ConfigCommands::Set { key, value } => {
            let mut config = Config::load().unwrap_or_default();
            config.set(&key, &value)?;
            println!("✅ Set {} = {}", key, value);
        }
        ConfigCommands::Get { key } => {
            let config = Config::load().unwrap_or_default();
            let value = config.get(&key)?;
            println!("{}: {}", key, value);
        }
        ConfigCommands::Show => {
            let config = Config::load().unwrap_or_default();
            println!("Configuration:");
            println!("  base_url: {}", config.base_url);
            println!("  output_format: {}", config.output_format);
            println!("  default_symbol: {}", config.default_symbol);
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
            let token = if interactive || (token.is_none() && token_file.is_none()) {
                println!("Enter JWT Token:");
                rpassword::prompt_password("Token: ")?.trim().to_string()
            } else if let Some(file) = token_file {
                std::fs::read_to_string(file)?.trim().to_string()
            } else {
                token.unwrap()
            };

            // Private key is optional - only needed for trading operations
            let private_key = if interactive {
                println!("\nEnter Ed25519 Private Key (Base58) - optional, press Enter to skip:");
                let key = rpassword::prompt_password("Private Key: ")?
                    .trim()
                    .to_string();
                if key.is_empty() {
                    None
                } else {
                    Some(key)
                }
            } else if let Some(file) = key_file {
                let key = std::fs::read_to_string(file)?.trim().to_string();
                if key.is_empty() {
                    None
                } else {
                    Some(key)
                }
            } else {
                private_key
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
        } => {
            let klines = client.get_kline(&symbol, &resolution, from, to).await?;

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

            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(funding_rates)),
                OutputFormat::Json => println!("{}", output::format_json(&funding_rates)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&funding_rates)?),
                OutputFormat::Quiet => {}
            }
        }
    }
    Ok(())
}

/// Handle stream commands
pub async fn handle_stream(command: StreamCommands) -> Result<()> {
    let ws = StandXWebSocket::new()?;

    match command {
        StreamCommands::Depth { symbol, levels } => {
            ws.subscribe("depth_book", Some(&symbol)).await;
            let mut rx = ws.connect().await?;

            println!("Streaming depth for {} (top {} levels)", symbol, levels);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                match msg {
                    WsMessage::DepthBook { data, .. } => {
                        println!("\n=== Order Book: {} ===", data.symbol);
                        println!("Asks:");
                        for (i, ask) in data.asks.iter().take(levels).enumerate() {
                            println!("  {}: {}", ask[0], ask[1]);
                        }
                        println!("Bids:");
                        for (i, bid) in data.bids.iter().take(levels).enumerate() {
                            println!("  {}: {}", bid[0], bid[1]);
                        }
                    }
                    _ => {}
                }
            }
        }
        StreamCommands::Ticker { symbol } => {
            ws.subscribe("price", Some(&symbol)).await;
            let mut rx = ws.connect().await?;

            println!("Streaming ticker for {}", symbol);
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                match msg {
                    WsMessage::Price { data } => {
                        println!(
                            "{} | Mark: {} | Index: {} | Last: {}",
                            data.time, data.mark_price, data.index_price, data.last_price
                        );
                    }
                    _ => {}
                }
            }
        }
        StreamCommands::Trades { symbol } => {
            println!("Trade streaming not yet implemented for {}", symbol);
        }
        StreamCommands::Account => {
            ws.subscribe("position", None).await;
            ws.subscribe("balance", None).await;
            ws.subscribe("order", None).await;
            let mut rx = ws.connect().await?;

            println!("Streaming account updates");
            println!("Press Ctrl+C to exit\n");

            while let Some(msg) = rx.recv().await {
                match msg {
                    WsMessage::Position { data } => {
                        println!("Position update: {}", serde_json::to_string(&data)?);
                    }
                    WsMessage::Balance { data } => {
                        println!("Balance update: {}", serde_json::to_string(&data)?);
                    }
                    WsMessage::Order { data } => {
                        println!("Order update: {}", serde_json::to_string(&data)?);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
