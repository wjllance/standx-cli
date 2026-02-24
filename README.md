# StandX CLI

A command-line interface tool for the StandX perpetual DEX API, written in Rust.

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## Features

- **Market Data**: Real-time and historical market data (tickers, order book, trades, klines, funding rates)
- **Account Management**: Query balances, positions, and order history
- **Order Management**: Create, cancel, and manage orders with full Ed25519 signature support
- **WebSocket Streaming**: Real-time data streams for price, depth, and account updates
- **Multiple Output Formats**: Table, JSON, and CSV output support
- **Secure Authentication**: JWT token with Ed25519 request signing

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/wjllance/standx-cli.git
cd standx-cli

# Build and install
cargo build --release

# The binary will be available at target/release/standx
```

### Prerequisites

- Rust 1.75 or higher
- A StandX account with API credentials

## Quick Start

### 1. Configure Authentication

Visit [https://standx.com/user/session](https://standx.com/user/session) to generate your API credentials:
- JWT Token
- Ed25519 Private Key (Base58 encoded)

Then login via CLI:

```bash
standx auth login --interactive
# Enter your JWT token and private key when prompted
```

### 2. View Market Data

```bash
# List all trading pairs
standx market symbols

# Get ticker for BTC-USD
standx market ticker BTC-USD

# View order book depth
standx market depth BTC-USD --limit 10

# Get recent trades
standx market trades BTC-USD --limit 20
```

### 3. Stream Real-time Data

```bash
# Stream order book updates
standx stream depth BTC-USD --levels 5

# Stream price ticker
standx stream ticker BTC-USD
```

## Commands Reference

### Authentication Commands

```bash
standx auth login --token <JWT> --private-key <KEY>    # Login with credentials
standx auth login --interactive                         # Interactive login
standx auth logout                                      # Clear credentials
standx auth status                                      # Check auth status
```

### Market Commands (Public API)

```bash
standx market symbols                                   # List all symbols
standx market ticker <SYMBOL>                           # Get symbol ticker
standx market tickers                                   # Get all tickers
standx market trades <SYMBOL> [--limit N]              # Recent trades
standx market depth <SYMBOL> [--limit N]               # Order book depth
standx market kline <SYMBOL> -r <RES> --from <TS> --to <TS>   # Kline data
standx market funding <SYMBOL> [--days N]              # Funding rate history
```

### Account Commands (Authenticated)

```bash
standx account balances                                 # Get account balances
standx account positions [--symbol <SYM>]              # Get positions
standx account orders [--symbol <SYM>]                 # Get open orders
standx account history [--symbol <SYM>] [--limit N]    # Order history
```

### Order Commands (Authenticated)

```bash
# Create limit order
standx order create <SYMBOL> <side> limit --qty <QTY> --price <PRICE>

# Create market order
standx order create <SYMBOL> <side> market --qty <QTY>

# Create order with stop-loss and take-profit
standx order create BTC-USD buy limit --qty 0.1 --price 63000 --sl-price 62000 --tp-price 65000

# Cancel order
standx order cancel <SYMBOL> --order-id <ID>

# Cancel all orders for symbol
standx order cancel-all <SYMBOL>
```

### Stream Commands

```bash
standx stream depth <SYMBOL> [--levels N]              # Stream order book
standx stream ticker <SYMBOL>                          # Stream price ticker
standx stream trades <SYMBOL>                          # Stream trades
standx stream account                                  # Stream account updates
```

### Configuration Commands

```bash
standx config init                                      # Initialize config
standx config set <KEY> <VALUE>                        # Set config value
standx config get <KEY>                                # Get config value
standx config show                                      # Show all config
```

## Output Formats

Use `--output` or `-o` flag to change output format:

```bash
standx market ticker BTC-USD --output json
standx account balances --output csv
standx market symbols --output table    # default
```

Available formats: `table`, `json`, `csv`, `quiet`

## API Endpoints

### Public Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/query_symbol_info` | GET | Trading pair information |
| `/api/query_symbol_market` | GET | Market data with funding rate |
| `/api/query_symbol_price` | GET | Price data |
| `/api/query_depth_book` | GET | Order book depth |
| `/api/query_recent_trades` | GET | Recent trades |
| `/api/kline/history` | GET | Kline/candlestick data |
| `/api/query_funding_rates` | GET | Funding rate history |

### Authenticated Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/query_balance` | GET | Account balances |
| `/api/query_positions` | GET | Position information |
| `/api/query_open_orders` | GET | Open orders |
| `/api/query_order_history` | GET | Order history |
| `/api/new_order` | POST | Create order |
| `/api/cancel_order` | POST | Cancel order |
| `/api/cancel_orders` | POST | Batch cancel orders |
| `/api/change_margin_mode` | POST | Change margin mode |

### WebSocket Channels

| Channel | Type | Description |
|---------|------|-------------|
| `depth_book` | Public | Order book updates |
| `price` | Public | Price ticker updates |
| `position` | Private | Position updates |
| `balance` | Private | Balance updates |
| `order` | Private | Order updates |

WebSocket URL: `wss://perps.standx.com/ws-stream/v1`

## Authentication

StandX uses JWT tokens with Ed25519 request signing:

1. **Token Acquisition**: Generate JWT + Ed25519 key pair from [StandX website](https://standx.com/user/session)
2. **Token Validity**: 7 days
3. **Request Signing**: Ed25519 signature for authenticated endpoints

### Request Headers

```
Authorization: Bearer <JWT_TOKEN>
x-request-sign-version: v1
x-request-id: <UUID>
x-request-timestamp: <UNIX_MS>
x-request-signature: <BASE64_SIGNATURE>
```

### Signature Format

```
message = "v1,request_id,timestamp,payload"
signature = ed25519_sign(private_key, message)
```

## Development

### Project Structure

```
standx-cli/
├── src/
│   ├── main.rs           # CLI entry point
│   ├── cli.rs            # CLI argument definitions
│   ├── commands.rs       # Command implementations
│   ├── lib.rs            # Library exports
│   ├── auth/             # Authentication module
│   │   ├── mod.rs        # Ed25519 signing
│   │   └── credentials.rs # Credential storage
│   ├── client/           # HTTP client
│   │   ├── mod.rs        # Public API
│   │   ├── account.rs    # Account API
│   │   └── order.rs      # Order API
│   ├── config.rs         # Configuration management
│   ├── error.rs          # Error types
│   ├── models.rs         # Data models
│   ├── output.rs         # Output formatting
│   └── websocket.rs      # WebSocket client
├── Cargo.toml
└── README.md
```

### Running Tests

```bash
cargo test
```

### Building Release

```bash
cargo build --release
```

## Configuration

Configuration is stored in:
- **Linux**: `~/.config/standx/config.toml`
- **macOS**: `~/Library/Application Support/standx/config.toml`
- **Windows**: `%APPDATA%\standx\config.toml`

Credentials are stored in:
- **Linux**: `~/.local/share/standx/credentials.enc`
- **macOS**: `~/Library/Application Support/standx/credentials.enc`
- **Windows**: `%APPDATA%\standx\credentials.enc`

## Troubleshooting

### Authentication Issues

```bash
# Check auth status
standx auth status

# Re-login if token expired
standx auth login --interactive
```

### API Errors

Use `--verbose` flag for detailed error information:

```bash
standx market ticker BTC-USD --verbose
```

### WebSocket Connection

The WebSocket client automatically handles:
- Connection failures (exponential backoff)
- Reconnection with resubscription
- Heartbeat monitoring
- Data stale detection

## License

This project is licensed under the MIT OR Apache-2.0 license.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Disclaimer

This is an unofficial CLI tool for StandX. Use at your own risk. Always verify orders before submission.
