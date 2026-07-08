//! CLI Command Integration Tests
//! Tests CLI commands using assert_cmd

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_cli_version() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("standx"))
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_cli_help() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("OpenClaw"))
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn test_cli_market_help() {
    let mut cmd = cargo_bin_cmd!("standx");
    cmd.args(["market", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("market"))
        .stdout(predicate::str::contains("symbols"))
        .stdout(predicate::str::contains("ticker"));
}
