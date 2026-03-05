# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0-rc.1] - 2026-03-04

### Added
- **Dashboard MVP** (#157)
  - Complete dashboard redesign with comfy-table formatting
  - Real-time order book depth display
  - Recent trades panel showing BUY/SELL activity
  - Enhanced account balance formatting with local timezone
  - Watch mode with graceful exit handling (Ctrl+C)
  - Instant refresh: fetch data before clearing screen
  - Dashboard title includes version number

### Changed
- **Dashboard Output Structure**
  - Reorganized display sections for improved clarity
  - Enhanced order display formatting
  - Better refresh label formatting
  - Cleaner table alignment

### Fixed
- **Dashboard Data Flow**
  - Improved dashboard and portfolio command handling
  - Enhanced trade handling and output formatting
  - Removed duplicate tests module in output.rs

## [0.6.3-rc.3] - 2026-03-03

### Fixed
- **Market Trades API Decoding** (#143)
  - Resolve trades API response decoding error
  - Fix trade history data parsing issues
- **Market Depth Table Alignment** (#144)
  - Fix output table formatting alignment
  - Improve depth display readability
- **Zero Quantity Positions** (#140)
  - Filter out zero-quantity positions from display
  - Cleaner portfolio view
- **Quiet Mode Flag** (#141)
  - Properly handle `-q` (quiet) flag
  - Suppress non-essential output when quiet mode is enabled
- **Test Environment** (#142)
  - Resolve test_from_env failure in CI
  - Improve test stability

## [0.6.3-rc.2] - 2026-03-03

### Added
- **Command Short Aliases** (#137)
  - Add short aliases for common commands (e.g., `s` for `snapshot`, `w` for `watch`)
  - Improve CLI usability and efficiency

### Fixed
- **Kline Timestamp Format** (#129)
  - Format timestamp to human-readable time
  - Improve readability of kline/candlestick data
- **Depth Spread Display** (#138)
  - Show spread in both dollar amount and percentage
  - Better market depth visualization
- **WebSocket Debug Logs** (#139)
  - Ensure debug logs only show with verbose flag
  - Clean up watch mode output

## [0.6.3-rc.1] - 2026-03-02

### Fixed
- **Auth Non-TTY Support** (#127)
  - Support non-TTY environments for login
  - Fix authentication issues in CI/automated environments
- **Dashboard+Portfolio Auth Handling** (#125)
  - Properly handle AuthRequired error for anonymous mode
  - Improve error messages for unauthenticated users

## [0.6.2] - 2026-03-01

### Fixed
- **Trade Model Field Mapping** (#113)
  - Correct Trade model field mapping for proper decoding
  - Fix trade history display issues

### Documentation
- **README Portfolio Command** (#115)
  - Add Portfolio command documentation to README
  - Include usage examples and options

## [0.6.1] - 2026-03-01

### Added
- **Dashboard Anonymous Mode** (#108)
  - Show login prompt when user is not authenticated
  - Support anonymous browsing of market data
- **Portfolio Base Functionality** (#106)
  - Add `portfolio` command with `snapshot` subcommand
  - Portfolio summary and performance view framework

### Fixed
- **Duplicate Portfolio Command** (#110)
  - Remove duplicate `Portfolio` enum variant in `Commands`
  - Fix merge conflict residue from PR #106
- **Dashboard Duplicate Call** (#109)
  - Avoid calling `get_balance()` twice in dashboard
  - Optimize data fetching logic

## [0.6.0] - 2026-03-01

### Added
- **Dashboard Command** (#35, #75, #83, #84, #100, #101)
  - Real-time trading dashboard with auto-refresh (`--watch`)
  - Symbol filtering (`--symbols`)
  - Table output formatting with color coding
  - Position, order, and market data in one view
- **Portfolio Command Base** (#105, #106)
  - Portfolio snapshot infrastructure
  - Framework for portfolio PnL analysis

### Fixed
- **Dashboard Symbol Filter** (#101)
  - Simplified symbol filter logic with `has_filter` variable
  - Changed `Ordering::SeqCst` to `Ordering::Relaxed` for AtomicBool

## [0.5.0] - 2026-03-01

### Added
- **Phase 3 Integration Tests** (#61, #62)
  - CLI command integration tests using `assert_cmd`
  - API flow tests with mock servers (`mockito`)
  - Output format tests (JSON, Table, CSV, Quiet)
  - Market data command tests
- **Phase 4 E2E Tests** (#32)
  - New user journey test suite
  - Trader daily workflow test suite
  - Automated end-to-end testing framework
- **Config Testability** (#66)
  - Added `load_from_path` for better testability
  - Environment variable override tests

### Fixed
- **E2E Test Parameter Format** (380bd8c)
  - Fixed market ticker command to use positional arg instead of `--symbol`

### Changed
- **Test Dependencies**
  - Added `tokio-test`, `mockito`, `tempfile`, `assert_cmd`, `predicates`
  - Improved test coverage and reliability

## [0.4.2] - 2026-02-26

### Fixed
- Position model updated (PR #24)
- Splash screen version (PR #23)

## [0.4.0] - 2026-02-26

### Added
- Telemetry module (PR #19)
- Improved authentication flow
- Splash screen improvements

## [0.3.6] - 2026-02-26

### Documentation
- Improved README authentication section

## [0.3.5] - 2026-02-26

### Changed
- OpenClaw Skill improvements
- Fixed GitHub Release binary upload in CI workflow
