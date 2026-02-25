# StandX Agent Toolkit

> **OpenClaw First. AI Agent Native. Trading Ecosystem Ready.**

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![OpenClaw](https://img.shields.io/badge/OpenClaw-First-blue.svg)](https://openclaw.ai)

**StandX Agent Toolkit** is a CLI designed for the AI Trading era—**OpenClaw First**, yet universally adaptable to any AI Agent that can execute commands.

We believe the future of trading is conversational. Your agent should trade as naturally as it chats. No complex APIs, no boilerplate—just intent to execution.

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

## 🎯 Why StandX Agent Toolkit?

### The Problem

You have an AI Agent (OpenClaw, Claude, AutoGPT, etc.). You want it to trade. But:
- ❌ Traditional trading tools are built for humans clicking buttons
- ❌ APIs require complex integration and parsing
- ❌ No bridge between natural language and execution

### The Solution

**Agent-First Design**—structured output, non-interactive, composable:

| Feature | Traditional Tools | StandX Agent Toolkit |
|---------|-------------------|----------------------|
| **Built For** | Human traders | **AI Agents** |
| **OpenClaw Integration** | Custom code | **Works out of the box** |
| **Output** | Pretty tables | **Structured JSON** |
| **Errors** | Text to parse | **Machine-readable** |
| **Workflow** | Interactive prompts | **100% scriptable** |
| **Other Agents** | Not supported | **CLI = Universal** |

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

### 2. Configure

```bash
# Get credentials from https://standx.com/user/session
standx auth login --token "$STANDX_JWT" --private-key "$STANDX_KEY"
```

### 3. Use With Your Agent

#### OpenClaw (Native)

```
You: What's the BTC price?
OpenClaw: [executes: standx market ticker BTC-USD --output json]
          BTC is trading at $65,000 (+2.3% today)

You: Buy 0.1 BTC at market price
OpenClaw: [executes: standx order create BTC-USD buy market --qty 0.1]
          ✅ Market order executed
          Bought 0.1 BTC at $65,001
```

#### Claude / Cursor / Any CLI-capable Agent

```python
# Same commands work everywhere
import subprocess

result = subprocess.run(
    ["standx", "market", "ticker", "BTC-USD", "--output", "json"],
    capture_output=True
)
data = json.loads(result.stdout)
```

---

## 🛠️ Integration Patterns

### Pattern 1: OpenClaw Native (Recommended)

OpenClaw calls StandX CLI directly via `exec`:

```python
# In OpenClaw
result = await exec("standx market ticker BTC-USD --output json")
price_data = json.loads(result.stdout)
# Agent parses and responds naturally
```

**Best for**: OpenClaw users who want seamless conversation-to-trading

### Pattern 2: Universal CLI

Any AI Agent that can execute shell commands:

```python
# LangChain
from langchain.tools import ShellTool

tool = ShellTool()
result = tool.run("standx account balances --output json")
```

```python
# AutoGPT
# Add to skills
os.system("standx order create BTC-USD buy market --qty 0.1")
```

**Best for**: Multi-platform agents, custom workflows

### Pattern 3: Future MCP (Optional)

When you need richer tool definitions:

```bash
# Coming soon
standx mcp serve
```

**Best for**: Complex multi-step workflows across multiple services

---

## 📋 Command Reference

### Market Data

```bash
# Price
standx market ticker BTC-USD --output json

# Order book
standx market depth BTC-USD --limit 10 --output json

# Recent trades
standx market trades BTC-USD --limit 20 --output json

# Funding rate
standx market funding BTC-USD --days 7 --output json
```

### Account

```bash
# Balance
standx account balances --output json

# Positions
standx account positions --symbol BTC-USD --output json

# Open orders
standx account orders --symbol BTC-USD --output json
```

### Trading

```bash
# Market order
standx order create BTC-USD buy market --qty 0.1

# Limit order
standx order create BTC-USD buy limit --qty 0.1 --price 64000

# With stop loss and take profit
standx order create BTC-USD buy limit --qty 0.1 --price 64000 \
  --sl-price 62000 --tp-price 68000

# Cancel
standx order cancel BTC-USD --order-id ord_xxx
standx order cancel-all BTC-USD
```

### Streaming

```bash
# Real-time data
standx stream ticker BTC-USD
standx stream depth BTC-USD --levels 5
standx stream account
```

---

## 💡 Use Cases

### 1. Natural Language Trading (OpenClaw)

```
You: "I want to long ETH with 0.5 size, entry at 3500"
OpenClaw: "I'll place a limit buy order for 0.5 ETH at $3,500. 
           Current price is $3,480. Confirm?"
You: "Yes"
OpenClaw: "✅ Order placed. Order ID: ord_eth_xxx"
```

### 2. Automated Strategy (Any Agent)

```python
# Grid trading bot
async def grid_trade():
    ticker = await exec("standx market ticker BTC-USD --output json")
    price = json.loads(ticker.stdout)["mark_price"]
    
    if price < lower_bound:
        await exec(f"standx order create BTC-USD buy limit --qty 0.01 --price {buy_price}")
```

### 3. Multi-Agent Coordination

```python
# Risk monitoring agent
while True:
    positions = await exec("standx account positions --output json")
    # Alert if exposure too high
    
# Execution agent
await exec("standx order create ...")
```

---

## 🗺️ Roadmap

### Phase 1: OpenClaw Excellence (Now)

**Goal**: Best-in-class OpenClaw integration

- [x] Structured JSON output
- [x] Non-interactive mode
- [ ] `--openclaw` optimized defaults
- [ ] Session persistence
- [ ] Batch execution

### Phase 2: Universal Agent Toolkit (1-2 months)

**Goal**: Seamless experience across all AI Agents

- [ ] Python SDK - `pip install standx-agent`
- [ ] Strategy templates (Grid, DCA, TWAP)
- [ ] Webhook callbacks
- [ ] MCP support (optional enhancement)

### Phase 3: AI Trading Ecosystem (3-6 months)

**Goal**: Define the standard for AI-native trading

- [ ] Multi-exchange abstraction
- [ ] Natural language strategy builder
- [ ] Agent marketplace
- [ ] Cross-agent coordination protocol

---

## 🤝 Comparison

| Tool | OpenClaw | Other Agents | Learning Curve |
|------|----------|--------------|----------------|
| **StandX Agent Toolkit** | 🟢 Native | 🟢 CLI = Universal | 🟢 Low |
| Hummingbot | 🔴 Complex | 🔴 Complex | 🔴 High |
| CCXT | 🟡 Wrapper needed | 🟡 Wrapper needed | 🟡 Medium |
| Hyperliquid SDK | 🟡 Integration needed | 🟡 Integration needed | 🟡 Medium |

---

## 🛡️ Safety Features

- **Structured errors** - Agents can handle errors programmatically
- **Dry-run mode** - Test without execution
- **Confirmation controls** - `--confirm` / `--no-confirm`
- **Rate limiting** - Built-in protection

---

## 📝 Philosophy

**OpenClaw First** — We optimize for the best OpenClaw experience first.

**Agent Native** — Every design decision prioritizes machine consumption over human readability.

**Ecosystem Ready** — CLI is the universal interface. Works with any agent, today.

**Future Proof** — MCP, SDKs, and advanced features come later. The foundation is solid.

---

## 📜 License

MIT OR Apache-2.0

---

**Built for the AI Trading era.**

*OpenClaw First. Agent Native. Ecosystem Ready.*

---

## 💰 Buy Me Some Tokens

If this tool helps your AI agent trade better, consider sponsoring some API tokens:

**EVM Address**: `0xAb3D58779dFC50BC84caA796003ABE31b5296210`

Your support helps keep the project maintained and improves AI-native trading infrastructure.

Thank you! 🤖💸
