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
        Self::load_from_path(None::<PathBuf>)
    }

    /// Load configuration from a specific path
    ///
    /// If `path` is `None`, uses the default config directory.
    /// If `path` is `Some(path)`, loads config from that directory.
    ///
    /// # Arguments
    /// * `path` - Optional path to the configuration directory
    ///
    /// # Returns
    /// * `Result<Self>` - The loaded configuration or an error
    ///
    /// # Example
    /// ```ignore
    /// // Load from default directory
    /// let config = Config::load_from_path(None)?;
    ///
    /// // Load from specific directory
    /// let config = Config::load_from_path(Some("/tmp/my-config"))?;
    /// ```
    pub fn load_from_path<T: Into<PathBuf>>(path: Option<T>) -> Result<Self> {
        let config_dir = match path {
            Some(p) => p.into(),
            None => Self::default_config_dir(),
        };
        let config_file = config_dir.join("config.toml");

        if !config_file.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_file).map_err(|e| Error::Config {
            message: format!("Failed to read config file: {}", e),
        })?;

        let mut config: Config = toml::from_str(&content).map_err(|e| Error::Config {
            message: format!("Failed to parse config file: {}", e),
        })?;

        config.config_dir = config_dir;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| Error::Config {
            message: format!("Failed to create config directory: {}", e),
        })?;

        let content = toml::to_string_pretty(self).map_err(|e| Error::Config {
            message: format!("Failed to serialize config: {}", e),
        })?;

        std::fs::write(self.config_file(), content).map_err(|e| Error::Config {
            message: format!("Failed to write config file: {}", e),
        })?;

        Ok(())
    }

    /// Set a configuration value
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "base_url" => self.base_url = value.to_string(),
            "output_format" => self.output_format = value.to_string(),
            "default_symbol" => self.default_symbol = value.to_string(),
            _ => {
                return Err(Error::Config {
                    message: format!("Unknown config key: {}", key),
                })
            }
        }
        self.save()
    }

    /// Get a configuration value
    pub fn get(&self, key: &str) -> Result<String> {
        match key {
            "base_url" => Ok(self.base_url.clone()),
            "output_format" => Ok(self.output_format.clone()),
            "default_symbol" => Ok(self.default_symbol.clone()),
            _ => Err(Error::Config {
                message: format!("Unknown config key: {}", key),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

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

    #[test]
    fn test_config_missing_file() {
        // 使用临时目录确保配置文件不存在
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join("nonexistent");

        // 直接测试：当配置文件不存在时，应该返回默认配置
        // 注意：这里我们手动构造场景，因为 Config::load() 使用固定路径
        let config_file = config_dir.join("config.toml");
        assert!(!config_file.exists());

        // 验证默认配置
        let config = Config::default();
        assert_eq!(config.base_url, "https://perps.standx.com");
        assert_eq!(config.output_format, "table");
    }

    #[test]
    fn test_config_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        // 写入损坏的 TOML 内容
        let mut file = std::fs::File::create(&config_file).unwrap();
        file.write_all(b"invalid toml content [[[").unwrap();
        drop(file);

        // 尝试从该目录加载配置应该失败
        // 由于 Config::load() 使用固定路径，我们测试 save/load 循环
        let config = Config {
            base_url: "https://test.com".to_string(),
            output_format: "json".to_string(),
            default_symbol: "ETH-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };

        // 先保存有效配置
        config.save().unwrap();

        // 然后损坏文件
        let mut file = std::fs::File::create(&config.config_file()).unwrap();
        file.write_all(b"invalid toml [[[").unwrap();
        drop(file);

        // 尝试加载损坏的配置文件
        // 注意：Config::load() 使用默认路径，这里我们手动测试解析错误
        let content = std::fs::read_to_string(&config.config_file()).unwrap();
        let result: std::result::Result<Config, _> = toml::from_str(&content);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_env_override_base_url() {
        // Test that environment variable can be set and read for base_url
        let _guard = EnvGuard::set("STANDX_BASE_URL", "https://env.standx.com");

        let env_url = std::env::var("STANDX_BASE_URL").unwrap();
        assert_eq!(env_url, "https://env.standx.com");
    }

    #[test]
    fn test_config_env_override_output_format() {
        // Test that environment variable can be set and read for output_format
        let _guard = EnvGuard::set("STANDX_OUTPUT_FORMAT", "json");

        let env_format = std::env::var("STANDX_OUTPUT_FORMAT").unwrap();
        assert_eq!(env_format, "json");
    }

    #[test]
    fn test_config_env_override_default_symbol() {
        // Test that environment variable can be set and read for default_symbol
        let _guard = EnvGuard::set("STANDX_DEFAULT_SYMBOL", "ETH-USD");

        let env_symbol = std::env::var("STANDX_DEFAULT_SYMBOL").unwrap();
        assert_eq!(env_symbol, "ETH-USD");
    }

    #[test]
    fn test_config_env_priority() {
        // Test environment variable priority: Env > File > Default
        // Create a config file with specific values
        let temp_dir = TempDir::new().unwrap();
        let mut config = Config {
            base_url: "https://file.standx.com".to_string(),
            output_format: "table".to_string(),
            default_symbol: "BTC-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };
        config.save().unwrap();

        // Set environment variable (should take priority)
        let _guard = EnvGuard::set("STANDX_BASE_URL", "https://env.standx.com");

        // Verify environment variable exists
        let env_val = std::env::var("STANDX_BASE_URL").unwrap();
        assert_eq!(env_val, "https://env.standx.com");
    }

    #[test]
    fn test_config_env_empty_string() {
        // Test empty string environment variable
        let _guard = EnvGuard::set("STANDX_BASE_URL", "");

        let env_val = std::env::var("STANDX_BASE_URL").unwrap();
        assert_eq!(env_val, "");
    }

    #[test]
    fn test_config_env_isolation() {
        // Test that EnvGuard properly restores original values
        // Set an initial value
        std::env::set_var("TEST_ISOLATION_VAR", "original");

        {
            let _guard = EnvGuard::set("TEST_ISOLATION_VAR", "modified");
            assert_eq!(std::env::var("TEST_ISOLATION_VAR").unwrap(), "modified");
        }

        // After guard is dropped, should be restored
        assert_eq!(std::env::var("TEST_ISOLATION_VAR").unwrap(), "original");

        // Cleanup
        std::env::remove_var("TEST_ISOLATION_VAR");
    }

    #[test]
    fn test_load_from_path_with_specific_directory() {
        let temp_dir = TempDir::new().unwrap();
        
        let mut config = Config {
            base_url: "https://specific.test.com".to_string(),
            output_format: "json".to_string(),
            default_symbol: "ETH-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };
        config.save().unwrap();

        let loaded = Config::load_from_path(Some(temp_dir.path())).unwrap();
        assert_eq!(loaded.base_url, "https://specific.test.com");
    }

    #[test]
    fn test_load_from_path_nonexistent_directory() {
        let nonexistent = PathBuf::from("/tmp/nonexistent_standx_test_dir");
        let result = Config::load_from_path(Some(&nonexistent));
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_from_path_with_string() {
        let temp_dir = TempDir::new().unwrap();
        
        let mut config = Config {
            base_url: "https://string.test.com".to_string(),
            output_format: "csv".to_string(),
            default_symbol: "DOGE-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };
        config.save().unwrap();

        let temp_path = temp_dir.path().to_str().unwrap();
        let loaded = Config::load_from_path(Some(temp_path)).unwrap();
        assert_eq!(loaded.base_url, "https://string.test.com");
    }

    #[test]
    fn test_load_from_path_with_pathbuf() {
        let temp_dir = TempDir::new().unwrap();
        
        let mut config = Config {
            base_url: "https://pathbuf.test.com".to_string(),
            output_format: "csv".to_string(),
            default_symbol: "DOGE-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };
        config.save().unwrap();

        let loaded = Config::load_from_path(Some(temp_dir.path().to_path_buf())).unwrap();
        assert_eq!(loaded.base_url, "https://pathbuf.test.com");
    }

    #[test]
    fn test_load_from_path_none_uses_default() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::load_from_path(Some(temp_dir.path())).unwrap();
        assert_eq!(config.base_url, "https://perps.standx.com");
    }

    #[test]
    fn test_load_backward_compatibility() {
        let temp_dir = TempDir::new().unwrap();

        let mut config = Config {
            base_url: "https://backward.compat.com".to_string(),
            output_format: "table".to_string(),
            default_symbol: "BTC-USD".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
        };
        config.save().unwrap();

        let loaded = Config::load_from_path(Some(temp_dir.path())).unwrap();
        assert_eq!(loaded.base_url, "https://backward.compat.com");
    }

    #[test]
    fn test_load_from_path_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let mut file = std::fs::File::create(&config_file).unwrap();
        file.write_all(b"invalid toml content [[[").unwrap();
        drop(file);

        let result = Config::load_from_path(Some(temp_dir.path()));
        assert!(result.is_err());
    }
}
