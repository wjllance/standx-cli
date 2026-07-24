use crate::cli::*;
use anyhow::Result;
use standx_sdk::auth::{credentials::ENV_JWT_TOKEN, Credentials, StandXSigner};

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

            if let Some(key) = private_key.as_deref() {
                if StandXSigner::from_base58(key).is_err() {
                    anyhow::bail!(
                        "Private key is malformed; re-check the Base58-encoded Ed25519 private key"
                    );
                }
            }

            let credentials = Credentials::new(token, private_key.clone());
            let expires_at = credentials.expires_at_string();
            let jwt_exp_unknown = credentials.jwt_exp().is_none();
            let token_is_expired = credentials.is_expired();
            credentials.save()?;

            println!("✅ Login successful!");
            println!("   Token expires at: {}", expires_at);
            if jwt_exp_unknown {
                println!("   ⚠️  Warning: Token does not look like a standard JWT.");
                println!(
                    "   Its real expiry is unknown; the expiry shown above is a local 7-day placeholder."
                );
            }
            if token_is_expired {
                println!(
                    "   ⚠️  Warning: Token is already expired; this login will not be usable."
                );
            }
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
                let source = if std::env::var(ENV_JWT_TOKEN).is_ok() {
                    "environment variable (STANDX_JWT)"
                } else {
                    "file"
                };
                let trading = if creds.private_key.is_empty() {
                    "unavailable (no private key)"
                } else {
                    "enabled"
                };

                if creds.is_expired() {
                    println!("❌ Token has expired!");
                    println!("   Token expired at: {}", expires_at);
                    println!("   Source: {}", source);
                    println!("   Trading: {}", trading);
                    println!("   Run 'standx auth login' to re-authenticate");
                } else {
                    println!("✅ Authenticated");
                    println!("   Token expires at: {}", expires_at);
                    println!("   Source: {}", source);
                    println!("   Trading: {}", trading);
                    let remaining = creds.remaining_seconds();
                    if remaining < 24 * 60 * 60 {
                        println!("   ⚠️  Warning: Token expires in less than 24 hours!");
                    } else {
                        println!("   Remaining: {} hours", remaining / 3600);
                    }
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
