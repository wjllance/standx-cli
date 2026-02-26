# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3] - 2026-02-26

### Added
- **OpenClaw Skill published**
  - Published `standx-cli` skill to ClawHub
  - Automatic installation via `clawhub install standx-cli`
  - Rich documentation and examples included

## [0.3.2] - 2026-02-26

### Fixed
- **K-line API response parsing**
  - Fixed K-line command failing with "HTTP request failed" error
  - Added `KlineResponse` struct to handle API response format
  - Now correctly parses `{"s": "ok", "t": [...], "o": [...], ...}` format
  - All resolution parameters working: 1, 5, 15, 30, 60, 240, 720, 1D, 1W, 1M

### Added
- **Comprehensive documentation**
  - Added `docs/` directory with 11 detailed guides
  - Quick start, authentication, market data, orders, trading, leverage, streaming
  - Output formats guide (table, JSON, CSV, quiet)
  - Special features (OpenClaw, Dry Run)
  - Troubleshooting guide
- **Git workflow guidelines** in `WORKFLOW.md`

## [0.3.1] - 2026-02-26

### Added
- **K-line command improvements** (ISSUE-2.1)
  - Support friendly time formats: Unix timestamp, ISO date (YYYY-MM-DD), relative time (1h, 1d, 7d)
  - Added `--limit` option as alternative to from/to
  - Added `parse_time_string()` helper function
- **Trade history command improvements** (ISSUE-4.1)
  - Same time format support as K-line
  - Default time range: last 24 hours
  - Empty result handling with informative message
- **Funding rate empty data handling** (ISSUE-2.2)
  - Added helpful message when no funding data available
  - Suggests alternative commands

### Fixed
- Confirmed Leverage and Margin APIs are working (ISSUE-4.2, ISSUE-4.3)
- Code formatting with `cargo fmt`

## [0.3.0] - 2026-02-26

### Added
- **New stream command structure** with 7 subcommands:
  - `stream price <symbol>` - Public price ticker
  - `stream depth <symbol>` - Public order book depth
  - `stream trade <symbol>` - Public trades
  - `stream order` - User order updates (authenticated)
  - `stream position` - User position updates (authenticated)
  - `stream balance` - User balance updates (authenticated)
  - `stream fills` - User fill/trade updates (authenticated)
- **Public channels without authentication** - price, depth, trade work without JWT
- **Verbose mode for WebSocket** - use `-v` flag to show debug messages
- **New WebSocket auth format** - `{ "auth": { "token": "Bearer ...", "streams": [...] } }`

### Fixed
- Fixed WebSocket channel names: `depth` → `depth_book`, `trades` → `public_trade`
- Fixed Trade struct to support WebSocket `side` field
- Fixed PriceData timestamp field mapping for WebSocket format
- Fixed WebSocket message parsing for all public channels

### Changed
- Split `stream account` into individual commands (order, position, balance, fills)
- WebSocket debug messages only shown in verbose mode
- Updated test report with 79% pass rate

## [0.2.0] - 2026-02-24

### Added
- WebSocket trade streaming support (`standx stream trades`)
- Automatic Homebrew formula update on release
- CI workflow for automated releases
- ROADMAP.md with future iteration plans
- CHANGELOG.md for version history

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
- Updated `Cargo.toml` version to 0.2.0
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

[Unreleased]: https://github.com/wjllance/standx-cli/compare/v0.3.3...HEAD
[0.3.3]: https://github.com/wjllance/standx-cli/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/wjllance/standx-cli/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/wjllance/standx-cli/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/wjllance/standx-cli/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/wjllance/standx-cli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/wjllance/standx-cli/releases/tag/v0.1.0
