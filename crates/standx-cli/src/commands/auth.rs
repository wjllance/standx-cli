use crate::cli::*;
use anyhow::Result;
use standx_sdk::auth::Credentials;

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
