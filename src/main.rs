mod cli;
mod commands;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else if cli.quiet {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    match cli.command {
        Commands::Config { command } => {
            commands::handle_config(command).await?;
        }
        Commands::Auth { command } => {
            commands::handle_auth(command).await?;
        }
        Commands::Market { command } => {
            commands::handle_market(command, cli.output).await?;
        }
        Commands::Account { command } => {
            commands::handle_account(command, cli.output).await?;
        }
        Commands::Order { command } => {
            commands::handle_order(command).await?;
        }
        Commands::Trade { command } => {
            tracing::info!("Trade command: {:?}", command);
            println!("Trade command not yet implemented");
        }
        Commands::Leverage { command } => {
            tracing::info!("Leverage command: {:?}", command);
            println!("Leverage command not yet implemented");
        }
        Commands::Margin { command } => {
            tracing::info!("Margin command: {:?}", command);
            println!("Margin command not yet implemented");
        }
        Commands::Stream { command } => {
            tracing::info!("Stream command: {:?}", command);
            println!("Stream command not yet implemented");
        }
    }

    Ok(())
}
