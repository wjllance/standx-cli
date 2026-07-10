//! Non-sensitive maker strategy configuration.

use crate::config::Config;
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Values are optional so an explicit CLI flag can override one field without
/// requiring every strategy default to be repeated in TOML.
#[derive(Debug, Default, Deserialize)]
pub(super) struct MakerFileConfig {
    pub spread_bps: Option<f64>,
    pub band_bps: Option<f64>,
    pub size: Option<f64>,
    pub levels: Option<u32>,
    pub level_step_bps: Option<f64>,
    pub refresh_bps: Option<f64>,
    pub interval: Option<u64>,
    pub max_position: Option<f64>,
    pub skew_bps: Option<f64>,
    pub inventory_exit_pct: Option<f64>,
    pub inventory_exit_qty: Option<f64>,
    pub max_divergence_bps: Option<f64>,
    pub vol_pause_bps: Option<f64>,
    pub vol_window: Option<u32>,
    pub alert_loss: Option<f64>,
    pub alert_inventory_pct: Option<f64>,
    pub alert_uptime: Option<f64>,
    pub no_ws: Option<bool>,
}

pub(super) fn load(path: Option<&Path>) -> Result<MakerFileConfig> {
    let path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| Config::default_config_dir().join("maker.toml"));
    if !path.exists() {
        if path.as_path() == Config::default_config_dir().join("maker.toml") {
            return Ok(MakerFileConfig::default());
        }
        return Err(anyhow::anyhow!(
            "maker config file not found: {}",
            path.display()
        ));
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content)
        .map_err(|error| anyhow::anyhow!("invalid maker config {}: {}", path.display(), error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_non_sensitive_strategy_file() {
        let config: MakerFileConfig =
            toml::from_str("spread_bps = 8\nmax_position = 0.02\nno_ws = true\n").unwrap();
        assert_eq!(config.spread_bps, Some(8.0));
        assert_eq!(config.max_position, Some(0.02));
        assert_eq!(config.no_ws, Some(true));
        assert_eq!(config.size, None);
    }
}
