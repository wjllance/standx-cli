# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-02-24

### Added
- WebSocket trade streaming support (`standx stream trades`)
- Automatic Homebrew formula update on release
- CI workflow for automated releases
- ROADMAP.md with future iteration plans

### Fixed
- Fixed order API and models for actual API format
  - Changed `time_in_force` to lowercase (`gtc`/`ioc`/`fok`)
  - Updated `Order` model fields (`qty`, `fill_qty`, `order_type`)
  - Added `OrderStatus::Open` variant
- Fixed WebSocket crypto provider issue (switched to native-tls)
- Fixed CI permissions for GitHub release
- Fixed all clippy warnings for `-D warnings`
- Fixed code formatting with `cargo fmt`

### Changed
- Updated `Cargo.toml` version to 0.1.2
- Updated repository URL in `Cargo.toml`

## [0.1.0] - 2026-02-22

### Added
- Initial release of StandX CLI
- Market data queries (symbols, ticker, trades, depth, kline, funding)
- Account management (balances, positions, orders, history)
- Order management (create, cancel, cancel-all)
- WebSocket streaming (ticker, depth, account)
- JWT + Ed25519 authentication
- Multiple output formats (table, json, csv)
- Homebrew support
- Comprehensive documentation (README, API docs, Homebrew guide)

[Unreleased]: https://github.com/wjllance/standx-cli/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/wjllance/standx-cli/compare/v0.1.0...v0.1.2
[0.1.0]: https://github.com/wjllance/standx-cli/releases/tag/v0.1.0
