//! CLI Output Format Integration Tests

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_market_symbols_json_output() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols", "--output", "json"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\"").or(predicate::str::contains("\"data\"")))
        .stdout(
            predicate::str::contains("BTC")
                .or(predicate::str::contains("ETH"))
                .or(predicate::str::contains("[]")),
        );
}

#[test]
fn test_market_symbols_table_output() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols", "--output", "table"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("SYMBOL").or(predicate::str::contains("Symbol")));
}

#[test]
fn test_market_symbols_csv_output() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols", "--output", "csv"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("symbol").or(predicate::str::contains("SYMBOL")));
}

#[test]
fn test_output_format_quiet() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "symbols", "--quiet"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::is_empty().or(predicate::str::contains("BTC")));
}
