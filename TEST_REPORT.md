# StandX CLI Test Report

**Test Date**: 2026-02-26  
**CLI Version**: 0.3.0  
**Test Environment**: Linux x86_64, Rust 1.93.1

---

## Test Overview

| Part | Name | Tests | Passed | Failed | Pass Rate |
|------|------|-------|--------|--------|-----------|
| Part 1 | Basic & Config | 8 | 6 | 2 | 75% |
| Part 2 | Public Market Data | 9 | 7 | 2 | 78% |
| Part 3 | Auth & Account | 6 | 6 | 0 | 100% |
| Part 4 | Orders & Trading | 8 | 5 | 3 | 63% |
| Part 5 | Streaming Data | 7 | 6 | 1 | 86% |
| Part 6 | Special Features | 6 | 5 | 1 | 83% |
| **Total** | | **44** | **35** | **9** | **80%** |

---

## Part 1: Basic & Config

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| Version info | `standx --version` | `standx 0.3.0` |
| Main help | `standx --help` | Shows all subcommands |
| Config help | `standx config --help` | Shows 4 subcommands |
| Show config | `standx config show` | 3 config items displayed |
| Get config item | `standx config get base_url` | `https://perps.standx.com` |
| Verbose mode | `standx -v config show` | Executes normally |

### ⚠️ Issues

| Issue | Description | Status |
|-------|-------------|--------|
| ISSUE-1.1 | JSON output format not working | 🔴 Pending |
| ISSUE-1.2 | Quiet mode not simplified | 🔴 Pending |

---

## Part 2: Public Market Data

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| Symbol list | `market symbols` | 4 trading pairs |
| BTC ticker | `market ticker BTC-USD` | Price displayed |
| ETH ticker | `market ticker ETH-USD` | Price displayed |
| All tickers | `market tickers` | 4 trading pairs |
| Order book depth | `market depth BTC-USD` | 10 levels of bids/asks |
| Recent trades | `market trades BTC-USD` | Trade records displayed |
| OpenClaw mode | `--openclaw market ticker` | JSON output works |

### ⚠️ Issues

| Issue | Description | Status |
|-------|-------------|--------|
| ISSUE-2.1 | K-line parameter format unfriendly | 🔴 Pending |
| ISSUE-2.2 | Funding rate returns empty data | 🔴 Pending |

---

## Part 3: Auth & Account

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| Auth help | `auth --help` | 3 subcommands |
| Auth status | `auth status` | Authenticated |
| Account help | `account --help` | 5 subcommands |
| Account balance | `account balances` | Balance displayed |
| Position query | `account positions` | Displayed normally |
| Current orders | `account orders` | Order list displayed |
| Order history | `account history` | Displayed normally |

---

## Part 4: Orders & Trading

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| Order help | `order --help` | 3 subcommands |
| Order create help | `order create --help` | Complete parameters |
| Trade help | `trade --help` | 1 subcommand |
| Leverage help | `leverage --help` | 2 subcommands |
| **Place order** | `order create BTC-USD buy limit` | **✅ Success** |
| **Query order** | `account orders` | **✅ Displayed** |
| **Cancel order** | `order cancel` | **✅ Cancelled** |

### ⚠️ Unimplemented Features

| Feature | Status | Note |
|---------|--------|------|
| `trade history` | ⚠️ | Not implemented |
| `leverage get/set` | ⚠️ | Not implemented |
| `margin transfer/mode` | ⚠️ | Not implemented |

---

## Part 5: Streaming Data (WebSocket)

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| Stream help | `stream --help` | 7 subcommands |
| **Stream price** | `stream price BTC-USD` | **✅ Normal output** |
| **Stream depth** | `stream depth BTC-USD` | **✅ Normal output** |
| **Stream trade** | `stream trade BTC-USD` | **✅ Normal output** |
| Stream order | `stream order` | Requires auth |
| Stream position | `stream position` | Requires auth |
| Stream balance | `stream balance` | Requires auth |
| Stream fills | `stream fills` | Requires auth |

### 🔧 Fixed Issues

| Issue | Fix |
|-------|-----|
| FIX-5.1 | Fixed channel names: `depth` → `depth_book`, `trades` → `public_trade` |
| FIX-5.2 | Fixed Trade struct to support WebSocket format |
| FIX-5.3 | Fixed PriceData timestamp field mapping |
| FIX-5.4 | Public channels work without token |
| FIX-5.5 | Added verbose mode for debug output |
| FIX-5.6 | Updated auth message format to `{ "auth": { "token": "Bearer ...", "streams": [...] } }` |

### Usage Examples

```bash
# Public channels - no auth required
standx stream price BTC-USD
standx stream depth BTC-USD
standx stream trade BTC-USD

# Public channels with debug output
standx -v stream price BTC-USD

# User channels - requires JWT token
export STANDX_JWT="your_jwt_token"
standx stream order
standx stream position
standx stream balance
standx stream fills
```

### ⚠️ Issues

| Issue | Description | Status |
|-------|-------------|--------|
| ISSUE-5.1 | User auth channels return `invalid token` | 🔴 Pending |

---

## Part 6: Special Features

### ✅ Passed Tests

| Test | Command | Result |
|------|---------|--------|
| OpenClaw mode | `--openclaw market ticker BTC-USD` | JSON output, AI-optimized |
| OpenClaw + dry-run | `--openclaw --dry-run order create` | Combined flags work |
| Dry run (table) | `--dry-run order create` | Shows warning for order commands |
| Dry run (JSON) | `-o json --dry-run order create` | Structured JSON output |
| Dry run market | `--dry-run market ticker` | Shows safe to execute |

### ⚠️ Partial Tests

| Test | Command | Result | Note |
|------|---------|--------|------|
| Auto-confirm flag | `--yes` | ✅ Flag exists | Not integrated with interactive prompts |
| Environment variable | `STANDX_AUTO_CONFIRM=true` | ✅ Recognized | No interactive prompts to skip currently |

### Feature Details

#### `--openclaw` Mode
- Forces JSON output regardless of `-o` setting
- Optimized for AI Agent consumption
- Example: `standx --openclaw market ticker BTC-USD`

#### `--dry-run` Mode
- Shows what would be executed without making changes
- Warns about financial impact for order/leverage/margin commands
- Safe commands (market, account, trade) marked as "read-only"
- Works with all output formats (table, json)

#### `--yes` / `STANDX_AUTO_CONFIRM`
- Flag exists and is parsed
- Environment variable supported
- Currently no interactive prompts in CLI (all commands are non-interactive)
- Reserved for future use when confirmation prompts are added

### ⚠️ Issues

| Issue | Description | Status |
|-------|-------------|--------|
| ISSUE-6.1 | `--yes` flag not integrated (no prompts to skip) | 🟡 Low Priority |

---

## Issue Summary

### Pending Issues

| ID | Description | Priority |
|----|-------------|----------|
| ISSUE-1.1 | JSON output format not working | Medium |
| ISSUE-1.2 | Quiet mode not simplified | Low |
| ISSUE-2.1 | K-line parameter format unfriendly | Medium |
| ISSUE-2.2 | Funding rate returns empty data | Low |
| ISSUE-4.1 | Trade history not implemented | Medium |
| ISSUE-4.2 | Leverage functions not implemented | Medium |
| ISSUE-4.3 | Margin functions not implemented | Low |
| ISSUE-5.1 | User auth channel token issue | Medium |
| ISSUE-6.1 | `--yes` flag not integrated (no prompts to skip) | Low |

### Fixed Issues

| ID | Description | Fix |
|----|-------------|-----|
| FIX-3.1 | Positions API parsing error | Changed to direct array parsing |
| FIX-3.2 | History API 404 | Changed to `/api/query_orders?status=filled` |
| FIX-3.3 | Orders API parsing error | Use `ApiListResponse` wrapper |
| FIX-4.1 | Private Key incorrect | Use correct Ed25519 key |
| FIX-5.1-5.6 | WebSocket streaming fixes | See Part 5 |

---

## Core Features Status

| Feature Module | Status | Note |
|----------------|--------|------|
| Basic commands | ✅ Complete | version, help, config |
| Public market data | ✅ Complete | symbols, ticker, depth, trades |
| Authentication | ✅ Normal | JWT + Private Key |
| Account queries | ✅ Normal | balances, positions, orders, history |
| Order management | ✅ Normal | create, cancel, query |
| Streaming (public) | ✅ Normal | price, depth, trade |
| Streaming (user) | ⚠️ Requires auth | order, position, balance, fills |
| Trade history | ⚠️ Not implemented | trade history |
| Leverage management | ⚠️ Not implemented | leverage get/set |
| Margin management | ⚠️ Not implemented | margin transfer/mode |
| OpenClaw mode | ✅ Complete | JSON output, AI-optimized |
| Dry run mode | ✅ Complete | Preview before execute |
| Auto-confirm | ⚠️ Partial | `--yes` flag exists, not fully integrated |

---

## Test Environment

```bash
# Auth credentials
export STANDX_JWT="eyJhbGciOiJFUzI1NiIsImtpZCI6IlhnaEJQSVNuN0RQVHlMcWJtLUVHVkVhOU1lMFpwdU9iMk1Qc2gtbUFlencifQ..."
export STANDX_PRIVATE_KEY="8RYHtn9RvCwgLyyeW5XurT4kVyZrDkN5B92P3FoLmsnb"

# API endpoints
base_url: https://perps.standx.com
websocket: wss://perps.standx.com/ws-stream/v1
```

---

*Report generated: 2026-02-26*
