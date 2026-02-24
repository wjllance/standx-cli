//! Command implementations

use crate::cli::*;
use standx_cli::auth::Credentials;
use standx_cli::client::StandXClient;
use standx_cli::config::Config;
use standx_cli::output;
use anyhow::Result;

/// Handle account commands
pub async fn handle_account(command: AccountCommands, output_format: OutputFormat) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        AccountCommands::Balances => {
            let balances = client.get_balance().await?;
            
            match output_format {
                OutputFormat::Table => println!("{}", output::format_table(balances)),
                OutputFormat::Json => println!("{}", output::format_json(&balances)?),
                OutputFormat::Csv => println!("{}", output::format_csv(&balances)?),
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
            let orders = client.get_order_history(symbol.as_deref(), Some(limit)).await?;
            
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
        AuthCommands::Login { token, token_file, private_key, key_file, interactive } => {
            let token = if interactive || (token.is_none() && token_file.is_none()) {
                println!("Enter JWT Token:");
                rpassword::prompt_password("Token: ")?.trim().to_string()
            } else if let Some(file) = token_file {
                std::fs::read_to_string(file)?.trim().to_string()
            } else {
                token.unwrap()
            };

            let private_key = if interactive || (private_key.is_none() && key_file.is_none()) {
                println!("Enter Ed25519 Private Key (Base58):");
                rpassword::prompt_password("Private Key: ")?.trim().to_string()
            } else if let Some(file) = key_file {
                std::fs::read_to_string(file)?.trim().to_string()
            } else {
                private_key.unwrap()
            };

            let credentials = Credentials::new(token, private_key);
            let expires_at = credentials.expires_at_string();
            credentials.save()?;
            
            println!("✅ Login successful!");
            println!("   Token expires at: {}", expires_at);
        }
        AuthCommands::Logout => {
            Credentials::delete()?;
            println!("✅ Logged out successfully");
        }
        AuthCommands::Status => {
            match Credentials::load() {
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
            }
        }
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
    }
    Ok(())
}
