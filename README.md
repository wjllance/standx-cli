# StandX CLI

A command-line interface tool for StandX perpetual DEX.

## Features

- 📊 **Market Data**: Query symbols, tickers, order book, trades, klines
- 🔐 **Authentication**: JWT + Ed25519 signature authentication
- 💼 **Account**: View balances, positions, and configuration
- 📝 **Orders**: Create, cancel, and manage orders
- 📈 **Streaming**: Real-time WebSocket data feeds
- 🎨 **Output Formats**: Table, JSON, CSV, or quiet mode

## Installation

### From Source

```bash
git clone https://github.com/yourusername/standx-cli.git
cd standx-cli
cargo build --release
```

The binary will be available at `target/release/standx`.

## Quick Start

### 1. Get API Credentials

1. Visit https://standx.com/user/session
2. Connect your wallet and login
3. Click "Generate API Token"
4. Save the JWT Token and Ed25519 Private Key

### 2. Configure Authentication

```bash
standx auth login --interactive
```

Or use files:
```bash
standx auth login --token-file ~/.standx/token.txt --key-file ~/.standx/key.txt
```

### 3. Query Market Data

```bash
# Get BTC-USD ticker
standx market ticker BTC-USD

# Get order book depth
standx market depth BTC-USD --limit 20

# Get recent trades
standx market trades BTC-USD --limit 50
```

### 4. Create an Order

```bash
# Limit buy order
standx order create BTC-USD buy limit --qty 0.1 --price 68000

# Market sell order
standx order create BTC-USD sell market --qty 0.1
```

## Commands

### Global Options

```
-c, --config <FILE>     Configuration file path
-o, --output <FORMAT>   Output format: table, json, csv, quiet
-v, --verbose           Verbose output
-q, --quiet             Quiet mode
-h, --help              Print help
-V, --version           Print version
```

### Command Overview

| Command | Description |
|---------|-------------|
| `config` | Configuration management |
| `auth` | Authentication management |
| `market` | Market data (public) |
| `account` | Account information (authenticated) |
| `order` | Order management (authenticated) |
| `trade` | Trade history (authenticated) |
| `leverage` | Leverage management (authenticated) |
| `margin` | Margin management (authenticated) |
| `stream` | Real-time data stream |

## Configuration

Configuration is stored in:
- Linux: `~/.config/standx/config.toml`
- macOS: `~/.config/standx/config.toml`
- Windows: `%APPDATA%\standx\config.toml`

Credentials are securely stored in the system keychain/keyring.

## Development

### Prerequisites

- Rust 1.75+
- Cargo

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test
```

## Documentation

- [API Documentation](docs/API.md)
- [Usage Guide](docs/USAGE.md)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Disclaimer

This software is provided as-is without any warranty. Use at your own risk. Trading cryptocurrency involves significant risk of loss.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
