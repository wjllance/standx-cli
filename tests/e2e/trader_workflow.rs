//! E2E Test: Trader Daily Workflow
//! Simulates a trader's daily routine

use assert_cmd::Command;

/// Test: Daily trader workflow
/// Steps: check positions → market analysis → place order → monitor
#[test]
#[ignore = "Requires TEST_TOKEN and TEST_PRIVATE_KEY environment variables"]
fn test_trader_daily_workflow() {
    // Step 1: Check account balance
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["account", "balance"]);
    cmd.assert().success();
    
    // Step 2: Check positions
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["position", "list"]);
    cmd.assert().success();
    
    // Step 3: Market analysis - get order book
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "depth", "--symbol", "BTC-USD"]);
    cmd.assert().success();
    
    // Step 4: Check funding rates
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "funding", "--symbol", "BTC-USD"]);
    cmd.assert().success();
    
    // Note: Trading operations require authentication
    // Skipped in automated CI
}

/// Test: Market data analysis workflow
#[test]
fn test_market_analysis_workflow() {
    // Get all symbols
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols"]);
    cmd.assert().success();
    
    // Get ticker for multiple symbols
    for symbol in ["BTC-USD", "ETH-USD"] {
        let mut cmd = Command::cargo_bin("standx").unwrap();
        cmd.args(["market", "ticker", "--symbol", symbol]);
        cmd.assert().success();
    }
}
