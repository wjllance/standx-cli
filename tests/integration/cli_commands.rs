//! CLI Command Integration Tests
//! Tests CLI commands using assert_cmd

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_cli_version() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("standx"))
        .stdout(predicate::str::contains("0.6"));
}

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("OpenClaw"))
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn test_cli_market_help() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(["market", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("market"))
        .stdout(predicate::str::contains("symbols"))
        .stdout(predicate::str::contains("ticker"));
}
