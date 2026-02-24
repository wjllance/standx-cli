mod cli;

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
            tracing::info!("Config command: {:?}", command);
            println!("Config command not yet implemented");
        }
        Commands::Auth { command } => {
            tracing::info!("Auth command: {:?}", command);
            println!("Auth command not yet implemented");
        }
        Commands::Market { command } => {
            tracing::info!("Market command: {:?}", command);
            println!("Market command not yet implemented");
        }
        Commands::Account { command } => {
            tracing::info!("Account command: {:?}", command);
            println!("Account command not yet implemented");
        }
        Commands::Order { command } => {
            tracing::info!("Order command: {:?}", command);
            println!("Order command not yet implemented");
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
