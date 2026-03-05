---
name: standx-cli
description: "Crypto trading CLI for StandX exchange v0.3.5. Use when users need to: (1) Query crypto market data (prices, order books, klines, funding rates), (2) Manage trading orders (create, cancel, view), (3) Check account balances, positions, and trade history, (4) Stream real-time market data via WebSocket, (5) Manage leverage and margin settings. Supports BTC, ETH, SOL, XRP and other trading pairs."
metadata:
  {
    "openclaw":
      {
        "emoji": "📈",
        "requires": { "bins": ["standx"] },
        "primaryCredential":
          {
            "kind": "env",
            "env": "STANDX_JWT",
            "description": "StandX JWT token from https://standx.com/user/session (valid 7 days)",
          },
        "optionalEnvVars":
          [
            {
              "name": "STANDX_PRIVATE_KEY",
              "description": "Ed25519 private key (Base58) for trading operations",
              "sensitive": true,
            },
          ],
        "install":
          [
            {
              "id": "brew",
              "kind": "brew",
              "formula": "wjllance/standx-cli/standx-cli",
              "bins": ["standx"],
              "label": "Install StandX CLI via Homebrew",
            },
            {
              "id": "github-linux",
              "kind": "script",
              "script": "curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.3.5/standx-v0.3.5-x86_64-unknown-linux-gnu.tar.gz && tar -xzf /tmp/standx.tar.gz -C /tmp && sudo mv /tmp/standx /usr/local/bin/ && sudo chmod +x /usr/local/bin/standx",
              "bins": ["standx"],
              "label": "Install StandX CLI on Linux",
            },
            {
              "id": "github-macos",
              "kind": "script",
              "script": "curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.3.5/standx-v0.3.5-aarch64-apple-darwin.tar.gz && tar -xzf /tmp/standx.tar.gz -C /tmp && sudo mv /tmp/standx /usr/local/bin/ && sudo chmod +x /usr/local/bin/standx",
              "bins": ["standx"],
              "label": "Install StandX CLI on macOS",
            },
            {
              "id": "manual",
              "kind": "script",
              "script": "git clone https://github.com/wjllance/standx-cli-skill.git ~/.openclaw/skills/standx-cli && echo 'Skill installed. Please install standx binary separately via Homebrew or direct download.'",
              "bins": ["standx"],
              "label": "Manual install (skill only, binary separate)",
            },
          ],
      },
  }
---

# StandX CLI Skill

StandX CLI is a crypto trading command-line tool for the StandX exchange.

## Installation

### Option 1: ClawHub (Recommended - Auto-install)

```bash
clawhub install standx-cli
```

### Option 2: Homebrew

```bash
brew tap wjllance/standx-cli
brew install standx-cli
```

### Option 3: Direct Download

```bash
# Linux x86_64
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.3.5/standx-v0.3.5-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx

# macOS Apple Silicon
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.3.5/standx-v0.3.5-aarch64-apple-darwin.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

### Option 4: Manual Install (Skill Only)

If you prefer to install the skill manually and manage the binary separately:

```bash
# Install skill
git clone https://github.com/wjllance/standx-cli-skill.git ~/.openclaw/skills/standx-cli

# Then install binary separately via Homebrew or direct download (see Option 2 or 3)
```

## Quick Start

Check installation:

```bash
standx --version
```

View BTC price:

```bash
standx market ticker BTC-USD
```

## Authentication

Most commands require authentication. StandX CLI supports multiple secure authentication methods.

### Environment Variables (Recommended)

The most secure way to authenticate. Credentials are not stored in shell history or command logs.

```bash
# Add to ~/.bashrc or ~/.zshrc
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_ed25519_private_key"

# Reload shell configuration
source ~/.bashrc
```

**Security Best Practices:**

- Never hardcode credentials in commands (appears in shell history)
- Never commit credentials to version control
- Set file permissions to 600 for any files containing credentials
- Rotate tokens regularly (they expire after 7 days)

### Get Credentials

Visit https://standx.com/user/session to generate:

- **JWT Token** (required) - Valid for 7 days, used for reading account data
- **Ed25519 Private Key** (optional but recommended) - Required for trading operations

### Verify Authentication

```bash
standx auth status
```

### Alternative Authentication Methods

#### Interactive Login

For first-time setup or testing:

```bash
standx auth login --interactive
```

#### File-based Login

For automation scripts where environment variables are not available:

```bash
# Store credentials in files with restricted permissions
echo "your_jwt_token" > ~/.standx_token
echo "your_private_key" > ~/.standx_key
chmod 600 ~/.standx_token ~/.standx_key

# Login using files
standx auth login --token-file ~/.standx_token --key-file ~/.standx_key
```

**⚠️ Avoid this in production:**

```bash
# DANGER: Credentials will be visible in shell history
standx auth login --token "your_token" --private-key "your_key"
```

### Logout

```bash
standx auth logout
```

## Market Data (No auth required)

### List trading pairs

```bash
standx market symbols
```

### Get ticker

```bash
standx market ticker BTC-USD
standx market ticker ETH-USD
```

### Order book depth

```bash
standx market depth BTC-USD --limit 10
```

### K-line (candlestick) data

```bash
# Last 24 hours, 1-hour candles
standx market kline BTC-USD -r 60 --from 1d

# Last 7 days, daily candles
standx market kline BTC-USD -r 1D --from 7d

# Specific date range
standx market kline BTC-USD -r 60 --from 2024-01-01 --to 2024-01-07
```

### Funding rate

```bash
standx market funding BTC-USD --days 7
```

## Account & Trading (Auth required)

### Account info

```bash
standx account balances
standx account positions
standx account orders
standx account history --limit 20
```

### Create order

```bash
# Limit buy
standx order create BTC-USD buy limit --qty 0.01 --price 60000

# Market sell
standx order create BTC-USD sell market --qty 0.01
```

### Cancel order

```bash
standx order cancel BTC-USD --order-id 123456
standx order cancel-all BTC-USD
```

### Trade history

```bash
standx trade history BTC-USD --from 7d
```

## Leverage & Margin (Auth required)

```bash
# Query leverage
standx leverage get BTC-USD

# Set leverage
standx leverage set BTC-USD 10

# Query margin mode
standx margin mode BTC-USD

# Set margin mode
standx margin mode BTC-USD --set isolated
```

## Real-time Streaming

### Public streams (No auth)

```bash
# Price stream
standx stream price BTC-USD

# Order book stream
standx stream depth BTC-USD --levels 5

# Trade stream
standx stream trade BTC-USD
```

### User streams (Auth required)

```bash
standx stream order     # Order updates
standx stream position  # Position updates
standx stream balance   # Balance updates
standx stream fills     # Fill updates
```

## Output Formats

```bash
# JSON output
standx -o json market ticker BTC-USD

# CSV export
standx -o csv market symbols > symbols.csv

# Quiet mode (just values)
standx -o quiet config get base_url
```

## Special Modes

### OpenClaw mode (AI-optimized JSON)

```bash
standx --openclaw market ticker BTC-USD
```

### Dry run (preview without executing)

```bash
standx --dry-run order create BTC-USD buy limit --qty 0.01 --price 60000
```

## References

- [API Documentation](references/api-docs.md)
- [Command Examples](references/examples.md)
- [Troubleshooting](references/troubleshooting.md)

## Links

- GitHub: https://github.com/wjllance/standx-cli
- Docs: https://github.com/wjllance/standx-cli/tree/main/docs
- Issues: https://github.com/wjllance/standx-cli/issues
