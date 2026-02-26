mod cli;
mod commands;
mod telemetry;

use clap::Parser;
use cli::{Cli, Commands, OutputFormat};
use telemetry::Telemetry;

/// Print cool splash screen
fn print_splash_screen() {
    // Only print if stdout is a terminal
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return;
    }

    println!();
    println!("    ╔══════════════════════════════════════════════════════════════════╗");
    println!("    ║                                                                  ║");
    println!("    ║     ███████╗████████╗ █████╗ ███╗   ██╗██████╗ ██╗  ██╗        ║");
    println!("    ║     ██╔════╝╚══██╔══╝██╔══██╗████╗  ██║██╔══██╗╚██╗██╔╝        ║");
    println!("    ║     ███████╗   ██║   ███████║██╔██╗ ██║██║  ██║ ╚███╔╝         ║");
    println!("    ║     ╚════██║   ██║   ██╔══██║██║╚██╗██║██║  ██║ ██╔██╗         ║");
    println!("    ║     ███████║   ██║   ██║  ██║██║ ╚████║██████╔╝██╔╝ ██╗        ║");
    println!("    ║     ╚══════╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═══╝╚═════╝ ╚═╝  ╚═╝        ║");
    println!("    ║                                                                  ║");
    println!("    ║              ⚡ StandX Agent Toolkit ⚡                           ║");
    println!("    ║                                                                  ║");
    println!(
        "    ║                    Version {}                                 ║",
        env!("CARGO_PKG_VERSION")
    );
    println!("    ║                                                                  ║");
    println!("    ╚══════════════════════════════════════════════════════════════════╝");
    println!();
}

#[tokio::main]
async fn main() {
    // Check if we should show splash screen BEFORE parsing args
    // Show splash only when: no args, or --help/-h
    let args: Vec<String> = std::env::args().collect();
    let should_show_splash =
        args.len() == 1 || (args.len() == 2 && (args[1] == "--help" || args[1] == "-h"));

    if should_show_splash {
        print_splash_screen();
    }

    let cli = Cli::parse();
    let mut telemetry = Telemetry::new();

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

    // Track command start
    let command_name = format!("{:?}", cli.command);
    let args: Vec<String> = std::env::args().collect();
    telemetry.track_command_start(&command_name, &args);

    // Handle dry run mode
    if cli.dry_run {
        let output = cli.output;
        match handle_dry_run(&cli.command, output).await {
            Ok(_) => {
                telemetry.track_command_complete(&command_name, true, None);
                std::process::exit(0);
            }
            Err(e) => {
                let boxed_error: Box<dyn std::error::Error> = Box::new(e);
                print_error(&boxed_error, output);
                telemetry.track_command_complete(
                    &command_name,
                    false,
                    Some(&boxed_error.to_string()),
                );
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

    // Execute command and handle errors
    match execute_command(cli.command, output, cli.verbose).await {
        Ok(_) => {
            telemetry.track_command_complete(&command_name, true, None);
        }
        Err(e) => {
            print_error(&e, output);
            telemetry.track_command_complete(&command_name, false, Some(&e.to_string()));
            std::process::exit(1);
        }
    }
}

/// Execute the command, converting anyhow errors to our Error type
async fn execute_command(
    command: Commands,
    output: OutputFormat,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Config { command } => {
            commands::handle_config(command, output).await?;
        }
        Commands::Auth { command } => {
            commands::handle_auth(command).await?;
        }
        Commands::Market { command } => {
            commands::handle_market(command, output).await?;
        }
        Commands::Account { command } => {
            commands::handle_account(command, output).await?;
        }
        Commands::Order { command } => {
            commands::handle_order(command).await?;
        }
        Commands::Trade { command } => {
            commands::handle_trade(command, output).await?;
        }
        Commands::Leverage { command } => {
            commands::handle_leverage(command, output).await?;
        }
        Commands::Margin { command } => {
            commands::handle_margin(command).await?;
        }
        Commands::Stream { command } => {
            commands::handle_stream(command, verbose).await?;
        }
    }
    Ok(())
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
#[allow(clippy::borrowed_box)]
fn print_error(error: &Box<dyn std::error::Error>, output: OutputFormat) {
    match output {
        OutputFormat::Json => {
            // Try to convert to our Error type for structured output
            if let Some(standx_err) = error.downcast_ref::<standx_cli::Error>() {
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&standx_err.to_json()).unwrap()
                );
            } else {
                // Fallback for other error types
                let error_json = serde_json::json!({
                    "error": {
                        "error_type": "UNKNOWN_ERROR",
                        "message": error.to_string()
                    },
                    "timestamp": chrono::Utc::now().to_rfc3339()
                });
                eprintln!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            }
        }
        _ => {
            eprintln!("❌ Error: {}", error);
        }
    }
}
