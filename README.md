# StandX for OpenClaw

> **The native trading companion for OpenClaw agents**

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![OpenClaw](https://img.shields.io/badge/OpenClaw-Ready-blue.svg)](https://openclaw.ai)

**StandX for OpenClaw** is a purpose-built CLI that transforms your OpenClaw agent into a professional crypto trader. No complex integration, no boilerplate code—just natural language to execution.

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│   You: "Check my BTC position"                                  │
│   ↓                                                             │
│   OpenClaw → StandX CLI → StandX API                            │
│   ↓                                                             │
│   You: "Long 0.1 BTC, stop loss at $62k"                        │
│   ↓                                                             │
│   ✅ Order executed in seconds                                  │
│                                                                 │
│   Your agent now trades as naturally as it converses.           │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## 🎯 Why OpenClaw + StandX?

### The Problem

You have OpenClaw. You want your agent to trade. But:
- ❌ Generic trading bots are built for humans, not agents
- ❌ APIs require complex authentication and parsing
- ❌ No seamless bridge between natural language and execution

### The Solution

StandX CLI is **designed for OpenClaw's execution model**:

| Feature | Generic Tools | StandX for OpenClaw |
|---------|---------------|---------------------|
| **Integration** | Custom code needed | Works out of the box |
| **Output** | Tables for humans | JSON for agents |
| **Errors** | Text to parse | Structured, actionable |
| **Workflow** | Interactive prompts | 100% scriptable |
| **Context** | None | Maintains session state |

---

## 🚀 Quick Start

### 1. Install

```bash
# macOS
brew tap wjllance/standx-cli
brew install standx-cli

# Or build from source
cargo install standx-cli
```

### 2. Configure (One-time)

```bash
# Get credentials from https://standx.com/user/session
standx auth login --token "$STANDX_JWT" --private-key "$STANDX_KEY"
```

### 3. Use in OpenClaw

Your agent can now execute trades:

```
You: What's the BTC price?
OpenClaw: [executes: standx market ticker BTC-USD --output json]
          BTC is trading at $65,000 (+2.3% today)

You: Buy 0.1 BTC at market price
OpenClaw: [executes: standx order create BTC-USD buy market --qty 0.1]
          ✅ Market order executed
          Bought 0.1 BTC at $65,001
          Order ID: ord_xxx

You: Set a stop loss at $62,000
OpenClaw: [executes: standx order create BTC-USD sell limit --qty 0.1 --price 62000]
          ✅ Stop loss order placed
          Will sell 0.1 BTC if price drops to $62,000
```

---

## 🛠️ OpenClaw Integration

### Direct Command Execution

OpenClaw can call StandX CLI directly via the `exec` tool:

```python
# In your OpenClaw session
result = await exec("standx market ticker BTC-USD --output json")
price_data = json.loads(result.stdout)
```

### Recommended Workflow

```
User Request
     ↓
OpenClaw parses intent
     ↓
Selects StandX command
     ↓
Executes via exec()
     ↓
Parses JSON output
     ↓
Natural language response
```

### Example: Grid Strategy Agent

```python
# Your OpenClaw agent running a grid strategy

async def grid_trade():
    # Check current price
    ticker = await exec("standx market ticker BTC-USD --output json")
    price = json.loads(ticker.stdout)["mark_price"]
    
    # Get open orders
    orders = await exec("standx account orders --symbol BTC-USD --output json")
    open_orders = json.loads(orders.stdout)
    
    # Logic: If no orders in grid range, place new ones
    if should_place_buy_order(price, open_orders):
        await exec(f"standx order create BTC-USD buy limit --qty 0.01 --price {buy_price}")
    
    if should_place_sell_order(price, open_orders):
        await exec(f"standx order create BTC-USD sell limit --qty 0.01 --price {sell_price}")
```

---

## 📋 Command Reference for OpenClaw

### Market Data (No auth required)

```bash
# Get price
standx market ticker BTC-USD --output json

# Order book
standx market depth BTC-USD --limit 10 --output json

# Recent trades
standx market trades BTC-USD --limit 20 --output json

# Funding rate
standx market funding BTC-USD --days 7 --output json
```

### Account (Auth required)

```bash
# Balance
standx account balances --output json

# Positions
standx account positions --symbol BTC-USD --output json

# Open orders
standx account orders --symbol BTC-USD --output json
```

### Trading (Auth required)

```bash
# Market order
standx order create BTC-USD buy market --qty 0.1

# Limit order
standx order create BTC-USD buy limit --qty 0.1 --price 64000

# With stop loss and take profit
standx order create BTC-USD buy limit --qty 0.1 --price 64000 \
  --sl-price 62000 --tp-price 68000

# Cancel order
standx order cancel BTC-USD --order-id ord_xxx

# Cancel all
standx order cancel-all BTC-USD
```

### Streaming (Real-time)

```bash
# Price stream
standx stream ticker BTC-USD

# Order book updates
standx stream depth BTC-USD --levels 5

# Account updates
standx stream account
```

---

## 💡 Use Cases

### 1. Natural Language Trading

```
You: "I want to long ETH with 0.5 size, entry at 3500"
OpenClaw: "I'll place a limit buy order for 0.5 ETH at $3,500. 
           Current price is $3,480, so this will execute when 
           the price rises to your entry. Confirm?"
You: "Yes"
OpenClaw: [places order] "✅ Order placed. Order ID: ord_eth_xxx"
```

### 2. Automated Monitoring

```
OpenClaw: "I'll monitor your BTC position. If it drops below 
           $60,000, I'll alert you and suggest hedging options."
[Every 5 minutes]
OpenClaw: [checks price] "BTC at $62,500. Position healthy."
```

### 3. Risk Management

```
You: "Set up a trailing stop for my BTC position"
OpenClaw: "Current BTC price: $65,000. Your position: +5% profit.
           I'll set a trailing stop at -3% from peak. 
           If BTC hits $63,050, I'll sell."
[Price rises to $68,000]
OpenClaw: "Trailing stop updated to $65,960 (3% below new peak)"
```

### 4. Multi-Step Strategies

```
You: "Execute a grid strategy on BTC from 60k to 70k"
OpenClaw: "Setting up grid:
           - 10 levels from $60,000 to $70,000
           - Each level: 0.01 BTC
           - Total exposure: 0.1 BTC
           Placing orders..."
[Places 10 buy orders and 10 sell orders]
OpenClaw: "✅ Grid active. I'll monitor and rebalance as orders fill."
```

---

## 🔧 Configuration for OpenClaw

### Environment Variables

```bash
# Add to your OpenClaw environment
export STANDX_JWT="your-jwt-token"
export STANDX_PRIVATE_KEY="your-private-key"
```

### Config File

```toml
# ~/.config/standx/config.toml
[openclaw]
auto_confirm = true        # Skip confirmations in agent mode
default_output = "json"    # Always JSON for parsing
show_raw_output = false    # Only show parsed results
```

---

## 📊 Comparison

| Tool | Built For | OpenClaw Integration | Learning Curve |
|------|-----------|---------------------|----------------|
| **StandX CLI** | OpenClaw agents | Native | 🟢 Low |
| Hummingbot | Human traders | Complex | 🔴 High |
| CCXT | Developers | Requires wrapper | 🟡 Medium |
| Hyperliquid SDK | Developers | Requires integration | 🟡 Medium |

---

## 🛡️ Safety Features

### For Agent Use

- **Structured errors** - Agent can parse and handle errors programmatically
- **Dry-run mode** - Test commands without execution
- **Confirmation prompts** - Critical actions require explicit confirmation
- **Rate limiting** - Built-in protection against accidental spam

### Example: Safe Execution

```bash
# Dry run first
standx order create BTC-USD buy market --qty 0.1 --dry-run
# Output: "Would buy 0.1 BTC at ~$65,000. Cost: ~$6,500"

# Then execute
standx order create BTC-USD buy market --qty 0.1
```

---

## 🗺️ Roadmap

### Now (Phase 1)
- [x] Core CLI with JSON output
- [x] Structured error handling
- [ ] **OpenClaw-optimized defaults**
- [ ] **Session state management**
- [ ] **Batch command execution**

### Next (Phase 2)
- [ ] **OpenClaw skill** - Native integration
- [ ] **Strategy templates** - Grid, DCA, TWAP
- [ ] **Webhook callbacks** - Event-driven agents
- [ ] **Python SDK** - `pip install standx-openclaw`

### Future (Phase 3)
- [ ] **Multi-exchange** - Unified interface
- [ ] **AI strategy builder** - Natural language to strategy
- [ ] **Agent marketplace** - Share strategies

---

## 🤝 Contributing

We welcome contributions that improve the OpenClaw experience:

- OpenClaw workflow examples
- Strategy templates
- Documentation improvements
- Bug reports

---

## 📜 License

MIT OR Apache-2.0

---

**Built for OpenClaw. Powered by StandX.**

*Your agent's trading journey starts here.*
