# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Maker bot: `standx maker run <SYMBOL>`** (alias `mk`) — two-sided quoting loop targeting SIP-5A community maker yield
  - Anti-flicker reconcile: quotes rest inside the eligibility band and only re-quote when mark drifts past `--refresh-bps`
  - Flags: `--spread-bps`, `--band-bps`, `--size`, `--levels`, `--level-step-bps`, `--refresh-bps`, `--interval`, `--max-position`
  - **Paper mode by default** (full loop, prints intended actions, no orders); `--live` implements real post-only quoting but is locked behind `STANDX_ENABLE_LIVE_MAKER=1` pending supervised production testing
  - Live safety rails: startup cancel-all, exchange open-orders as reconciliation truth, cancel-all-with-retry + verification on exit, fail-safe stop after 3 consecutive API errors
  - JSON-lines output for agents (`--output json` / `--openclaw`)
  - Pure quoting/reconcile core in `standx_sdk::maker` with 26 unit tests
  - Volatility circuit breaker (`--vol-pause-bps`, default 0/off; `--vol-window`, default 12): halts quoting (pulls all resting quotes) when the mark's peak-to-trough range over the window reaches the threshold, and resumes once it falls below half that (hysteresis — the move must roll out of the window). Guards against getting run over during fast moves. Halted cycles surface as `⚡HALT` on the human line, `halted`/`vol_bps` in the JSON summary, and a count in the exit summary
  - Risk alerts (`AlertMonitor`): edge-triggered threshold alerts on the financial risks the telemetry previously only displayed — `--alert-loss` (mark-to-market PnL floor), `--alert-inventory-pct` (position reaches % of `--max-position`), `--alert-uptime` (two-sided uptime floor, after warmup). Each opt-in (0 disables), fires once on breach and once on recovery (no per-cycle spam). Delivered to stderr / JSON always, and to an optional `--alert-webhook` URL (POST with a Slack/Discord-friendly `text` field, spawned fire-and-forget so a slow endpoint never stalls the loop)
  - Inventory skew (`--skew-bps`, default 0/off): shifts the quote center by current position so the reducing side quotes nearer mark and the growing side further, turning `--max-position` from a hard brake into gradual mean reversion. The anti-flicker anchor generalizes from "mark at placement" to "quote center at placement," so the same re-quote rule reacts to both mark drift and inventory skew
  - Paper-mode fill simulation: a resting quote crossed by the touch is treated as filled and folds its signed qty into a simulated position, so inventory (and thus skew) is now observable in paper mode without going live. Fills surface as `FILL` lines / `fill` JSON events and in the exit summary (fills count + ending position)
  - Session telemetry (`standx_sdk::maker::MakerStats`): mark-to-market PnL, favorable spread capture (bps/fill), two-sided uptime %, fill count/volume, and max inventory. Surfaced as `pnl=` on each human cycle line, `pnl`/`uptime_pct`/`avg_capture_bps`/`fills_total` in the JSON cycle summary, and a stats block in the exit summary. Works in paper (exact simulated fills) and live (fills inferred from position deltas). Turns skew/spread/refresh tuning into a measured loop
  - WebSocket market feed (price + depth on one connection) with automatic REST fallback when the feed is warming up or stale; `--no-ws` forces REST polling
  - Early re-quote: wakes before the interval elapses when the cached mark has already drifted past `--refresh-bps` (only fires when a re-quote would happen anyway — no added flicker; 1s min gap)
  - mark/mid divergence guard (`--max-divergence-bps`, default 25): skips the cycle without touching resting quotes when mark price and book mid disagree
  - Error classification: post-only (ALO) would-cross rejections and cancels of already-gone orders are treated as normal events (logged, re-quoted next cycle) instead of counting toward the 3-consecutive-error fail-safe — only transient failures (network, 5xx) trip it
  - Partial-fill tolerance: a partially-filled resting order keeps its identity (adopted by side + price, qty ≤ placed) and holds its remainder instead of being cancelled as an unknown order
- `TimeInForce::Alo` (post-only / add-liquidity-only), matching the backend enum; `standx order create --tif ALO` now supported
- Block trade commands: `standx block list` / `standx block watch`

### Changed
- **Workspace split: `standx-sdk` extracted as an independent crate**
  - `crates/standx-sdk` (v0.1.0): REST client, WebSocket streams, models, auth/signing, errors — reusable by any Rust agent/bot; zero presentation dependencies by default (table rendering behind the optional `tabled` feature)
  - `crates/standx-cli` (v0.8.0): the `standx` binary — commands, output formatting, config, telemetry; re-exports the SDK surface for backward compatibility
  - Release artifacts unchanged (binary name `standx`, same CI/homebrew/install.sh flow)
- Removed unused dependencies: `comfy-table`, `once_cell`, `config`, `keyring` (and the vestigial `no-keyring` feature)

### Fixed
- Kline streaming: handle symbol/interval in parent message
- `order create`: removed `-q` short flag (collided with global `--quiet`; clap panics on the collision in debug builds). Use `--qty`.
- Deflaked env-var tests (config + credentials) by serializing them with a lock
- Version integration test no longer hardcodes the version number
- Wired the previously-orphaned `tests/unit/` tree into a compiled test target

## [0.7.0] - 2026-03-05

### Added
- **Dashboard MVP** (#157)
  - Complete dashboard redesign with comfy-table formatting
  - Real-time order book depth display
  - Recent trades panel showing BUY/SELL activity
  - Enhanced account balance formatting with local timezone
  - Watch mode with graceful exit handling (Ctrl+C)
  - Instant refresh: fetch data before clearing screen
  - Dashboard title includes version number
- **Automated Pre-release Workflow** (#167)
  - Push tag to auto-create Pre-release
  - Multi-platform binary builds (macOS ARM64, Linux x86_64/ARM64)
  - Automatic checksum generation

### Changed
- **Dashboard Output Structure**
  - Reorganized display sections for improved clarity
  - Enhanced order display formatting
  - Better refresh label formatting
  - Cleaner table alignment
- **CI/CD Improvements**
  - Auto-prerelease for RC/Beta/Alpha versions
  - Homebrew update only for stable releases

### Fixed
- **Dashboard Data Flow**
  - Improved dashboard and portfolio command handling
  - Enhanced trade handling and output formatting
  - Removed duplicate tests module in output.rs

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
