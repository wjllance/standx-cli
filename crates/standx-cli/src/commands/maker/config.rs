//! Non-sensitive maker strategy configuration.

use crate::config::Config;
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Values are optional so an explicit CLI flag can override one field without
/// requiring every strategy default to be repeated in TOML.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub stop_loss: Option<f64>,
    pub alert_loss: Option<f64>,
    pub alert_inventory_pct: Option<f64>,
    pub alert_position_change_pct: Option<f64>,
    pub alert_uptime: Option<f64>,
    pub alert_equity_below: Option<f64>,
    pub alert_margin_below: Option<f64>,
    pub no_ws: Option<bool>,
    pub order_response_reconnect_attempts: Option<u32>,
    pub order_response_reconnect_backoff: Option<u64>,
    pub account_stream_reconnect_attempts: Option<u32>,
    pub account_stream_reconnect_backoff: Option<u64>,
    pub recovery_incidents_per_window: Option<u32>,
    pub recovery_window_secs: Option<u64>,
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
        let config: MakerFileConfig = toml::from_str(
            "spread_bps = 8\nmax_position = 0.02\nalert_position_change_pct = 20\nno_ws = true\norder_response_reconnect_attempts = 3\norder_response_reconnect_backoff = 2\naccount_stream_reconnect_attempts = 3\naccount_stream_reconnect_backoff = 2\nrecovery_incidents_per_window = 3\nrecovery_window_secs = 3600\n",
        )
        .unwrap();
        assert_eq!(config.spread_bps, Some(8.0));
        assert_eq!(config.max_position, Some(0.02));
        assert_eq!(config.alert_position_change_pct, Some(20.0));
        assert_eq!(config.no_ws, Some(true));
        assert_eq!(config.order_response_reconnect_attempts, Some(3));
        assert_eq!(config.order_response_reconnect_backoff, Some(2));
        assert_eq!(config.account_stream_reconnect_attempts, Some(3));
        assert_eq!(config.account_stream_reconnect_backoff, Some(2));
        assert_eq!(config.recovery_incidents_per_window, Some(3));
        assert_eq!(config.recovery_window_secs, Some(3600));
        assert_eq!(config.size, None);
    }

    #[test]
    fn parses_stop_loss_and_account_floor_fields() {
        let config: MakerFileConfig =
            toml::from_str("stop_loss = 25\nalert_equity_below = 100\nalert_margin_below = 40\n")
                .unwrap();
        assert_eq!(config.stop_loss, Some(25.0));
        assert_eq!(config.alert_equity_below, Some(100.0));
        assert_eq!(config.alert_margin_below, Some(40.0));
    }

    #[test]
    fn example_keeps_active_inventory_exit_disabled() {
        let config: MakerFileConfig = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/maker.toml"
        )))
        .unwrap();

        assert_eq!(config.inventory_exit_pct, Some(0.0));
        assert_eq!(config.inventory_exit_qty, Some(0.0));
        assert_eq!(config.order_response_reconnect_attempts, Some(3));
        assert_eq!(config.order_response_reconnect_backoff, Some(2));
        assert_eq!(config.account_stream_reconnect_attempts, Some(3));
        assert_eq!(config.account_stream_reconnect_backoff, Some(2));
        assert_eq!(config.recovery_incidents_per_window, Some(3));
        assert_eq!(config.recovery_window_secs, Some(3600));
    }

    #[test]
    fn rejects_unknown_keys_so_a_typo_does_not_silently_disable_a_guard() {
        // `alert_los` is a typo for `alert_loss`; without deny_unknown_fields it
        // parses fine and the loss guard stays off without warning.
        let error = toml::from_str::<MakerFileConfig>("alert_los = 3.0\n").unwrap_err();
        assert!(
            error.to_string().contains("alert_los"),
            "error should name the offending key: {error}"
        );
    }

    #[test]
    fn xag_example_enables_twenty_percent_position_jump_alert() {
        let config: MakerFileConfig = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/maker-xag-100u.toml"
        )))
        .unwrap();

        assert_eq!(config.max_position, Some(0.8));
        assert_eq!(config.alert_position_change_pct, Some(20.0));
    }

    #[test]
    fn conservative_bnb_example_preserves_xag_notional_scale() {
        let config: MakerFileConfig = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/maker-bnb-100u-conservative.toml"
        )))
        .unwrap();

        assert_eq!(config.size, Some(0.02));
        assert_eq!(config.max_position, Some(0.08));
        assert_eq!(config.inventory_exit_pct, Some(50.0));
        assert_eq!(config.inventory_exit_qty, Some(0.02));
    }

    #[test]
    fn conservative_tsla_example_preserves_xag_notional_scale() {
        let config: MakerFileConfig = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/maker-tsla-100u-conservative.toml"
        )))
        .unwrap();

        assert_eq!(config.size, Some(0.03));
        assert_eq!(config.max_position, Some(0.12));
        assert_eq!(config.inventory_exit_pct, Some(50.0));
        assert_eq!(config.inventory_exit_qty, Some(0.03));
    }
}
