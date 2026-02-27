//! Credential management for StandX CLI

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Environment variable names
pub const ENV_JWT_TOKEN: &str = "STANDX_JWT";
pub const ENV_PRIVATE_KEY: &str = "STANDX_PRIVATE_KEY";

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

    /// Load credentials from environment variables or file
    /// Priority: Environment variables > File storage
    pub fn load() -> Result<Self> {
        // First, try to load from environment variables
        if let Ok(creds) = Self::from_env() {
            return Ok(creds);
        }

        // Fall back to file storage
        Self::from_file()
    }

    /// Load credentials from environment variables
    pub fn from_env() -> Result<Self> {
        let token = std::env::var(ENV_JWT_TOKEN).map_err(|_| Error::AuthRequired {
            message: "Environment variable STANDX_JWT not set".to_string(),
            resolution: "Set STANDX_JWT environment variable or run 'standx auth login'"
                .to_string(),
        })?;

        let private_key = std::env::var(ENV_PRIVATE_KEY).unwrap_or_default();

        // Environment credentials don't have expiration tracking
        // Assume they're managed externally
        Ok(Self {
            token,
            private_key,
            created_at: chrono::Utc::now().timestamp(),
            validity_seconds: 365 * 24 * 60 * 60, // 1 year (effectively no expiration for env vars)
        })
    }

    /// Load credentials from file storage
    fn from_file() -> Result<Self> {
        let file_path = Self::credentials_file()?;

        if !file_path.exists() {
            return Err(Error::AuthRequired {
                message: "No credentials found".to_string(),
                resolution: "Set STANDX_JWT environment variable or run 'standx auth login'"
                    .to_string(),
            });
        }

        let encrypted = std::fs::read(&file_path).map_err(|e| Error::Config {
            message: format!("Failed to read credentials: {}", e),
        })?;

        let json = Self::xor_decrypt(&encrypted);

        let credentials: Credentials = serde_json::from_str(&json).map_err(|e| Error::Config {
            message: format!("Failed to parse credentials: {}", e),
        })?;

        Ok(credentials)
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
            .ok_or_else(|| Error::Config {
                message: "Could not determine data directory".to_string(),
            })
    }

    /// Get credentials file path
    fn credentials_file() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("credentials.enc"))
    }

    /// Save credentials to file (simple encryption - in production use proper keyring)
    pub fn save(&self) -> Result<()> {
        let data_dir = Self::data_dir()?;
        std::fs::create_dir_all(&data_dir).map_err(|e| Error::Config {
            message: format!("Failed to create data directory: {}", e),
        })?;

        // Simple XOR encryption with a fixed key (for basic protection)
        // In production, use proper keyring or OS credential store
        let json = serde_json::to_string(self).map_err(|e| Error::Config {
            message: format!("Failed to serialize credentials: {}", e),
        })?;

        let encrypted = Self::xor_encrypt(&json);

        std::fs::write(Self::credentials_file()?, encrypted).map_err(|e| Error::Config {
            message: format!("Failed to write credentials: {}", e),
        })?;

        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata =
                std::fs::metadata(Self::credentials_file()?).map_err(|e| Error::Config {
                    message: format!("Failed to get metadata: {}", e),
                })?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600); // Owner read/write only
            std::fs::set_permissions(Self::credentials_file()?, permissions).map_err(|e| {
                Error::Config {
                    message: format!("Failed to set permissions: {}", e),
                }
            })?;
        }

        Ok(())
    }

    /// Delete stored credentials
    pub fn delete() -> Result<()> {
        let file_path = Self::credentials_file()?;

        if file_path.exists() {
            std::fs::remove_file(file_path).map_err(|e| Error::Config {
                message: format!("Failed to delete credentials: {}", e),
            })?;
        }

        Ok(())
    }

    /// Check if credentials exist (in file or environment)
    pub fn exists() -> bool {
        // Check environment first
        if std::env::var(ENV_JWT_TOKEN).is_ok() {
            return true;
        }
        // Then check file
        Self::credentials_file()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Check if credentials are from environment variables
    pub fn is_from_env(&self) -> bool {
        // If created_at is very recent (within last second), likely from env
        // This is a heuristic - env credentials are created on each load
        let now = chrono::Utc::now().timestamp();
        now - self.created_at < 2
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

    /// Helper struct to temporarily set environment variables
    /// Restores original value (or removes if not set) when dropped
    struct EnvGuard {
        key: String,
        original_value: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let original_value = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                original_value,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(val) => std::env::set_var(&self.key, val),
                None => std::env::remove_var(&self.key),
            }
        }
    }

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

    #[test]
    fn test_from_env() {
        // Set environment variables using EnvGuard for automatic cleanup
        let _token_guard = EnvGuard::set(ENV_JWT_TOKEN, "env_test_token");
        let _key_guard = EnvGuard::set(ENV_PRIVATE_KEY, "env_test_key");

        let creds = Credentials::from_env().unwrap();
        assert_eq!(creds.token, "env_test_token");
        assert_eq!(creds.private_key, "env_test_key");
        // EnvGuard automatically cleans up when dropped
    }

    #[test]
    fn test_from_env_missing() {
        // Ensure env vars are not set
        std::env::remove_var(ENV_JWT_TOKEN);
        std::env::remove_var(ENV_PRIVATE_KEY);

        // Ensure no credentials file exists
        let _ = Credentials::delete();

        let result = Credentials::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_credentials_encryption_roundtrip() {
        // Test that encryption and decryption are inverse operations
        let test_cases = vec![
            "simple_token",
            "token_with_special_chars!@#$%",
            "", // Empty string
        ];

        for original in test_cases {
            let encrypted = Credentials::xor_encrypt(original);
            let decrypted = Credentials::xor_decrypt(&encrypted);
            assert_eq!(original, decrypted, "Failed for input: {}", original);
            // Encrypted should be different from original (unless empty)
            if !original.is_empty() {
                assert_ne!(encrypted, original.as_bytes());
            }
        }

        // Test with large data to verify encryption handles arbitrary length
        // 100 repetitions creates a ~1600 character string to test buffer handling
        let long_token = "very_long_token_".repeat(100);
        let encrypted = Credentials::xor_encrypt(&long_token);
        let decrypted = Credentials::xor_decrypt(&encrypted);
        assert_eq!(long_token, decrypted);
    }

    #[test]
    fn test_credentials_save_load_roundtrip() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let creds = Credentials {
            token: "test_token_123".to_string(),
            private_key: "test_key_456".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            validity_seconds: 7 * 24 * 60 * 60,
        };

        // Save to temp location
        let file_path = temp_dir.path().join("credentials.enc");
        let encrypted = Credentials::xor_encrypt(&serde_json::to_string(&creds).unwrap());
        std::fs::write(&file_path, encrypted).unwrap();

        // Load and verify
        let loaded_encrypted = std::fs::read(&file_path).unwrap();
        let loaded_json = Credentials::xor_decrypt(&loaded_encrypted);
        let loaded: Credentials = serde_json::from_str(&loaded_json).unwrap();

        assert_eq!(loaded.token, creds.token);
        assert_eq!(loaded.private_key, creds.private_key);
        assert_eq!(loaded.created_at, creds.created_at);
    }

    #[test]
    fn test_credentials_corrupted_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("credentials.enc");

        // Write corrupted data (not valid encrypted JSON)
        std::fs::write(&file_path, b"corrupted_data_not_valid").unwrap();

        // Attempt to decrypt should produce garbage, JSON parse should fail
        let loaded_encrypted = std::fs::read(&file_path).unwrap();
        let loaded_json = Credentials::xor_decrypt(&loaded_encrypted);
        let result: std::result::Result<Credentials, _> = serde_json::from_str(&loaded_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_jwt_expiration_calculation() {
        // Test that expiration is calculated correctly
        let now = chrono::Utc::now().timestamp();
        let creds = Credentials {
            token: "test_token".to_string(),
            private_key: "test_key".to_string(),
            created_at: now,
            validity_seconds: 3600, // 1 hour
        };

        // Should not be expired immediately
        assert!(!creds.is_expired());

        // Remaining seconds should be close to 3600
        let remaining = creds.remaining_seconds();
        assert!(remaining > 3590 && remaining <= 3600);

        // Expires at string should contain UTC
        let expires_str = creds.expires_at_string();
        assert!(expires_str.contains("UTC"));
    }

    #[test]
    fn test_jwt_expired_token() {
        // Test expired token detection
        let now = chrono::Utc::now().timestamp();
        let mut creds = Credentials {
            token: "test_token".to_string(),
            private_key: "test_key".to_string(),
            created_at: now - 7200, // 2 hours ago
            validity_seconds: 3600, // 1 hour validity
        };

        // Should be expired (2 hours > 1 hour validity)
        assert!(creds.is_expired());
        assert_eq!(creds.remaining_seconds(), 0);

        // Test with token expired by 1 second
        creds.created_at = now - 3601;
        assert!(creds.is_expired());
    }

    #[test]
    fn test_jwt_token_format() {
        // Test that token is stored as-is
        let test_tokens = vec![
            "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.test",
            "simple_token_123",
            "token-with-dashes_and_underscores",
            "Bearer token_with_special_chars!@#$%",
        ];

        for token in test_tokens {
            let creds = Credentials::new(token.to_string(), None);
            assert_eq!(creds.token, token);
        }
    }
}
