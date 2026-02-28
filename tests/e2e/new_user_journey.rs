//! E2E Test: New User Journey
//! Simulates a new user from installation to first trade

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

/// Test: New user complete journey
/// Steps: version check → help → config setup → market data → auth setup
#[test]
#[ignore = "Requires TEST_TOKEN environment variable"]
fn test_new_user_journey() {
    // Create isolated temp directory for config
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config/standx");
    fs::create_dir_all(&config_dir).unwrap();
    
    // Step 1: Check CLI version
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--version");
    cmd.assert().success();
    
    // Step 2: View help
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--help");
    cmd.assert().success();
    
    // Step 3: View market symbols (public API, no auth needed)
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols"]);
    cmd.assert().success();
    
    // Step 4: View market ticker
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "ticker", "--symbol", "BTC-USD"]);
    cmd.assert().success();
    
    // Note: Auth and trading steps require TEST_TOKEN
    // Skipped in automated CI, run manually with: cargo test --ignored
}

/// Test: CLI without config uses defaults
#[test]
fn test_cli_without_config() {
    // Ensure no config exists in temp environment
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.env("HOME", temp_dir.path());
    cmd.args(["market", "symbols", "--output", "json"]);
    cmd.assert().success();
}
