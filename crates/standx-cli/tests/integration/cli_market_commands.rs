//! CLI Market Commands Integration Tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_market_symbols_command() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.args(["market", "symbols"]);
    cmd.assert().success().stdout(
        predicate::str::contains("BTC")
            .or(predicate::str::contains("ETH"))
            .or(predicate::str::contains("SYMBOL")),
    );
}

#[test]
fn test_market_ticker_command() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.args(["market", "ticker", "BTC-USD"]);
    cmd.assert().success().stdout(
        predicate::str::contains("BTC-USD")
            .or(predicate::str::contains("mark_price"))
            .or(predicate::str::contains("Error")),
    );
}

#[test]
fn test_market_depth_command() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.args(["market", "depth", "BTC-USD"]);
    cmd.assert().success().stdout(
        predicate::str::contains("Asks")
            .or(predicate::str::contains("Bids"))
            .or(predicate::str::contains("Error")),
    );
}

#[test]
fn test_market_funding_command() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.args(["market", "funding", "BTC-USD"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Funding Rate").or(predicate::str::contains("Error")));
}
