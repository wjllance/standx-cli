//! Configuration management for StandX CLI

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// API base URL
    pub base_url: String,

    /// Default output format
    pub output_format: String,

    /// Default trading symbol
    pub default_symbol: String,

    /// Configuration directory
    #[serde(skip)]
    pub config_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: "https://perps.standx.com".to_string(),
            output_format: "table".to_string(),
            default_symbol: "BTC-USD".to_string(),
            config_dir: Self::default_config_dir(),
        }
    }
}

impl Config {
    /// Get default configuration directory
    pub fn default_config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("standx")
    }

    /// Get configuration file path
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Load configuration from file
    pub fn load() -> Result<Self> {
        let config_dir = Self::default_config_dir();
        let config_file = config_dir.join("config.toml");

        if !config_file.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_file)
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        let mut config: Config = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("Failed to parse config file: {}", e)))?;

        config.config_dir = config_dir;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)
            .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;

        let content = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(self.config_file(), content)
            .map_err(|e| Error::Config(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    /// Set a configuration value
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "base_url" => self.base_url = value.to_string(),
            "output_format" => self.output_format = value.to_string(),
            "default_symbol" => self.default_symbol = value.to_string(),
            _ => return Err(Error::Config(format!("Unknown config key: {}", key))),
        }
        self.save()
    }

    /// Get a configuration value
    pub fn get(&self, key: &str) -> Result<String> {
        match key {
            "base_url" => Ok(self.base_url.clone()),
            "output_format" => Ok(self.output_format.clone()),
            "default_symbol" => Ok(self.default_symbol.clone()),
            _ => Err(Error::Config(format!("Unknown config key: {}", key))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.base_url, "https://perps.standx.com");
        assert_eq!(config.output_format, "table");
        assert_eq!(config.default_symbol, "BTC-USD");
    }

    #[test]
    fn test_config_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config {
            base_url: "https://test.standx.com".to_string(),
            output_format: "json".to_string(),
            default_symbol: "ETH-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };

        // Save config
        config.save().unwrap();

        // Verify file exists
        assert!(config.config_file().exists());

        // Read and verify content
        let content = std::fs::read_to_string(config.config_file()).unwrap();
        assert!(content.contains("https://test.standx.com"));
        assert!(content.contains("json"));
    }

    #[test]
    fn test_set_get() {
        let mut config = Config::default();

        config.set("base_url", "https://test.com").unwrap();
        assert_eq!(config.get("base_url").unwrap(), "https://test.com");

        config.set("output_format", "json").unwrap();
        assert_eq!(config.get("output_format").unwrap(), "json");

        assert!(config.set("unknown_key", "value").is_err());
        assert!(config.get("unknown_key").is_err());
    }
}
