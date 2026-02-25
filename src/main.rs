mod cli;
mod commands;

use clap::Parser;
use cli::{Cli, Commands, OutputFormat};

#[tokio::main]
async fn main() {
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

    // Handle dry run mode
    if cli.dry_run {
        let output = cli.output;
        match handle_dry_run(&cli.command, output).await {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                print_error(e, output);
                std::process::exit(1);
            }
        }
    }

    // Set default output to JSON in OpenClaw mode
    let output = if cli.openclaw {
        OutputFormat::Json
    } else {
        cli.output
    };

    // Execute command
    let result = match cli.command {
        Commands::Config { command } => {
            commands::handle_config(command).await
        }
        Commands::Auth { command } => {
            commands::handle_auth(command).await
        }
        Commands::Market { command } => {
            commands::handle_market(command, output).await
        }
        Commands::Account { command } => {
            commands::handle_account(command, output).await
        }
        Commands::Order { command } => {
            commands::handle_order(command).await
        }
        Commands::Trade { command } => {
            tracing::info!("Trade command: {:?}", command);
            println!("Trade command not yet implemented");
            Ok(())
        }
        Commands::Leverage { command } => {
            tracing::info!("Leverage command: {:?}", command);
            println!("Leverage command not yet implemented");
            Ok(())
        }
        Commands::Margin { command } => {
            tracing::info!("Margin command: {:?}", command);
            println!("Margin command not yet implemented");
            Ok(())
        }
        Commands::Stream { command } => {
            commands::handle_stream(command).await
        }
    };

    if let Err(e) = result {
        print_error(e, output);
        std::process::exit(1);
    }
}

/// Handle dry run mode - show what would be executed
async fn handle_dry_run(command: &Commands, output: OutputFormat) -> Result<(), standx_cli::Error> {
    let description = match command {
        Commands::Config { .. } => "Would modify configuration",
        Commands::Auth { .. } => "Would authenticate with StandX",
        Commands::Market { .. } => "Would fetch market data (read-only, safe to execute)",
        Commands::Account { .. } => "Would fetch account data (read-only, safe to execute)",
        Commands::Order { .. } => "⚠️  WOULD CREATE/CANCEL ORDER - FINANCIAL IMPACT",
        Commands::Trade { .. } => "Would fetch trade history (read-only, safe to execute)",
        Commands::Leverage { .. } => "⚠️  WOULD MODIFY LEVERAGE - POSITION IMPACT",
        Commands::Margin { .. } => "⚠️  WOULD MODIFY MARGIN - POSITION IMPACT",
        Commands::Stream { .. } => "Would start real-time data stream",
    };

    let dry_run_info = serde_json::json!({
        "dry_run": true,
        "command": format!("{:?}", command),
        "description": description,
        "would_execute": !matches!(command, Commands::Order { .. } | Commands::Leverage { .. } | Commands::Margin { .. }),
        "note": "Remove --dry-run to execute"
    });

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&dry_run_info).unwrap());
        }
        _ => {
            println!("🔍 DRY RUN MODE");
            println!("{}", dry_run_info["description"].as_str().unwrap());
            if !dry_run_info["would_execute"].as_bool().unwrap() {
                println!("⚠️  This command would have financial impact");
            }
            println!("\nRemove --dry-run to execute for real");
        }
    }

    Ok(())
}

/// Print error in appropriate format
fn print_error(error: standx_cli::Error, output: OutputFormat) {
    match output {
        OutputFormat::Json => {
            eprintln!("{}", serde_json::to_string_pretty(&error.to_json()).unwrap());
        }
        _ => {
            eprintln!("❌ Error: {}", error);
            if let Some(action) = error.suggested_action() {
                eprintln!("💡 Suggested action: {}", action);
            }
        }
    }
}
