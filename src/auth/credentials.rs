//! Credential management for StandX CLI

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Stored credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// JWT token
    pub token: String,

    /// Ed25519 private key (Base58) - optional for read-only access
    #[serde(default)]
    pub private_key: String,

    /// Token creation timestamp (for expiration tracking)
    pub created_at: i64,

    /// Token validity in seconds (default: 7 days)
    pub validity_seconds: i64,
}

impl Credentials {
    /// Create new credentials (private key is optional)
    pub fn new(token: String, private_key: Option<String>) -> Self {
        Self {
            token,
            private_key: private_key.unwrap_or_default(),
            created_at: chrono::Utc::now().timestamp(),
            validity_seconds: 7 * 24 * 60 * 60, // 7 days
        }
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now > self.created_at + self.validity_seconds
    }

    /// Get remaining validity in seconds
    pub fn remaining_seconds(&self) -> i64 {
        let now = chrono::Utc::now().timestamp();
        let expires_at = self.created_at + self.validity_seconds;
        (expires_at - now).max(0)
    }

    /// Get expiration date as string
    pub fn expires_at_string(&self) -> String {
        let expires = self.created_at + self.validity_seconds;
        let datetime =
            chrono::DateTime::from_timestamp(expires, 0).unwrap_or_else(chrono::Utc::now);
        datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    }

    /// Get data directory
    fn data_dir() -> Result<PathBuf> {
        dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))
            .map(|d| d.join("standx"))
            .ok_or_else(|| Error::Config("Could not determine data directory".to_string()))
    }

    /// Get credentials file path
    fn credentials_file() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("credentials.enc"))
    }

    /// Save credentials to file (simple encryption - in production use proper keyring)
    pub fn save(&self) -> Result<()> {
        let data_dir = Self::data_dir()?;
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| Error::Config(format!("Failed to create data directory: {}", e)))?;

        // Simple XOR encryption with a fixed key (for basic protection)
        // In production, use proper keyring or OS credential store
        let json = serde_json::to_string(self)
            .map_err(|e| Error::Config(format!("Failed to serialize credentials: {}", e)))?;

        let encrypted = Self::xor_encrypt(&json);

        std::fs::write(Self::credentials_file()?, encrypted)
            .map_err(|e| Error::Config(format!("Failed to write credentials: {}", e)))?;

        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(Self::credentials_file()?)
                .map_err(|e| Error::Config(format!("Failed to get metadata: {}", e)))?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600); // Owner read/write only
            std::fs::set_permissions(Self::credentials_file()?, permissions)
                .map_err(|e| Error::Config(format!("Failed to set permissions: {}", e)))?;
        }

        Ok(())
    }

    /// Load credentials from file
    pub fn load() -> Result<Self> {
        let file_path = Self::credentials_file()?;

        if !file_path.exists() {
            return Err(Error::AuthRequired);
        }

        let encrypted = std::fs::read(&file_path)
            .map_err(|e| Error::Config(format!("Failed to read credentials: {}", e)))?;

        let json = Self::xor_decrypt(&encrypted);

        let credentials: Credentials = serde_json::from_str(&json)
            .map_err(|e| Error::Config(format!("Failed to parse credentials: {}", e)))?;

        Ok(credentials)
    }

    /// Delete stored credentials
    pub fn delete() -> Result<()> {
        let file_path = Self::credentials_file()?;

        if file_path.exists() {
            std::fs::remove_file(file_path)
                .map_err(|e| Error::Config(format!("Failed to delete credentials: {}", e)))?;
        }

        Ok(())
    }

    /// Check if credentials exist
    pub fn exists() -> bool {
        Self::credentials_file()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Simple XOR encryption (for basic protection)
    fn xor_encrypt(data: &str) -> Vec<u8> {
        const KEY: &[u8] = b"standx-cli-v1-key";
        data.bytes()
            .enumerate()
            .map(|(i, b)| b ^ KEY[i % KEY.len()])
            .collect()
    }

    /// Simple XOR decryption
    fn xor_decrypt(data: &[u8]) -> String {
        const KEY: &[u8] = b"standx-cli-v1-key";
        data.iter()
            .enumerate()
            .map(|(i, b)| b ^ KEY[i % KEY.len()])
            .map(|b| b as char)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_new() {
        let mut creds = Credentials::new("test_token".to_string(), Some("test_key".to_string()));

        assert_eq!(creds.token, "test_token");
        assert_eq!(creds.private_key, "test_key");
        assert!(!creds.is_expired());
        assert!(creds.remaining_seconds() > 0);

        // Test expires_at_string returns a valid string
        let expires = creds.expires_at_string();
        assert!(expires.contains("UTC"));
    }

    #[test]
    fn test_credentials_without_private_key() {
        let creds = Credentials::new(
            "test_token".to_string(),
            None, // No private key
        );

        assert_eq!(creds.token, "test_token");
        assert_eq!(creds.private_key, ""); // Empty string
        assert!(!creds.is_expired());
    }

    #[test]
    fn test_expiration() {
        let mut creds = Credentials::new("test_token".to_string(), Some("test_key".to_string()));

        // Set created_at to 8 days ago
        creds.created_at = chrono::Utc::now().timestamp() - 8 * 24 * 60 * 60;

        assert!(creds.is_expired());
        assert_eq!(creds.remaining_seconds(), 0);
    }

    #[test]
    fn test_xor_encryption() {
        let original = "test data 123";
        let encrypted = Credentials::xor_encrypt(original);
        let decrypted = Credentials::xor_decrypt(&encrypted);

        assert_eq!(original, decrypted);
        assert_ne!(encrypted, original.as_bytes());
    }
}
