use crate::cli::*;
use crate::output;
use anyhow::Result;
use standx_sdk::client::StandXClient;

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
