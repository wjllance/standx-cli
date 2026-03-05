# StandX CLI Command Examples

## Market Data Examples

### Get all trading pairs
```bash
standx market symbols
```

### Get BTC price in different formats
```bash
# Table (default)
standx market ticker BTC-USD

# JSON
standx -o json market ticker BTC-USD

# CSV
standx -o csv market ticker BTC-USD

# Quiet (just the price)
standx -o quiet market ticker BTC-USD
```

### Get order book
```bash
# Default 10 levels
standx market depth BTC-USD

# Custom 5 levels
standx market depth BTC-USD --limit 5

# Best bid/ask only
standx -o quiet market depth BTC-USD
```

### K-line data with time formats
```bash
# Relative time - last 24 hours, hourly
standx market kline BTC-USD -r 60 --from 1d

# Relative time - last 7 days, daily
standx market kline BTC-USD -r 1D --from 7d

# ISO date range
standx market kline BTC-USD -r 60 --from 2024-01-01 --to 2024-01-07

# Unix timestamps
standx market kline BTC-USD -r 60 --from 1704067200 --to 1706659200

# Limit results
standx market kline BTC-USD -r 60 --from 7d -l 10
```

### Different resolutions
```bash
# 1 minute
standx market kline BTC-USD -r 1 --from 1h

# 5 minutes
standx market kline BTC-USD -r 5 --from 6h

# 15 minutes
standx market kline BTC-USD -r 15 --from 1d

# 1 hour
standx market kline BTC-USD -r 60 --from 7d

# 4 hours
standx market kline BTC-USD -r 240 --from 30d

# 1 day
standx market kline BTC-USD -r 1D --from 90d

# 1 week
standx market kline BTC-USD -r 1W --from 1y
```

## Trading Examples

### Create orders
```bash
# Limit buy
standx order create BTC-USD buy limit --qty 0.01 --price 60000

# Limit sell
standx order create BTC-USD sell limit --qty 0.01 --price 70000

# Market buy
standx order create BTC-USD buy market --qty 0.01

# Market sell
standx order create BTC-USD sell market --qty 0.01

# With stop loss and take profit
standx order create BTC-USD buy limit \
  --qty 0.01 \
  --price 60000 \
  --sl-price 55000 \
  --tp-price 70000

# IOC (Immediate or Cancel)
standx order create BTC-USD buy limit --qty 0.01 --price 60000 --tif IOC

# Reduce only
standx order create BTC-USD sell limit --qty 0.01 --price 65000 --reduce-only
```

### Cancel orders
```bash
# Cancel specific order
standx order cancel BTC-USD --order-id 123456

# Cancel all orders for symbol
standx order cancel-all BTC-USD
```

### View orders
```bash
# Open orders
standx account orders

# Open orders for specific symbol
standx account orders --symbol BTC-USD

# Order history
standx account history

# Order history with limit
standx account history --limit 50
```

### Trade history
```bash
# Last 24 hours
standx trade history BTC-USD --from 1d

# Last 7 days
standx trade history BTC-USD --from 7d

# Date range
standx trade history BTC-USD --from 2024-01-01 --to 2024-01-31

# Export to CSV
standx -o csv trade history BTC-USD --from 30d > trades.csv
```

## Account Examples

### Check balances
```bash
standx account balances
```

### Check positions
```bash
# All positions
standx account positions

# Specific symbol
standx account positions --symbol BTC-USD
```

## Leverage & Margin Examples

### Query leverage
```bash
standx leverage get BTC-USD
```

### Set leverage
```bash
standx leverage set BTC-USD 10
standx leverage set BTC-USD 20
```

### Margin mode
```bash
# Query
standx margin mode BTC-USD

# Set to isolated
standx margin mode BTC-USD --set isolated

# Set to cross
standx margin mode BTC-USD --set cross
```

## Streaming Examples

### Public streams
```bash
# Price stream
standx stream price BTC-USD

# Order book stream
standx stream depth BTC-USD --levels 5

# Trade stream
standx stream trade BTC-USD
```

### User streams (requires auth)
```bash
# Order updates
standx stream order

# Position updates
standx stream position

# Balance updates
standx stream balance

# Fill updates
standx stream fills
```

## Special Features

### OpenClaw mode (AI-optimized)
```bash
standx --openclaw market ticker BTC-USD
standx --openclaw account balances
standx --openclaw trade history BTC-USD --from 7d
```

### Dry run (preview)
```bash
# Preview order creation
standx --dry-run order create BTC-USD buy limit --qty 0.01 --price 60000

# Preview leverage change
standx --dry-run leverage set BTC-USD 20
```

### Combined options
```bash
# OpenClaw + Dry Run
standx --openclaw --dry-run order create BTC-USD buy limit --qty 0.01 --price 60000

# JSON output + limit
standx -o json market kline BTC-USD -r 60 --from 1d -l 5
```

## Scripting Examples

### Get current price in script
```bash
PRICE=$(standx -o quiet market ticker BTC-USD)
echo "Current BTC price: $PRICE"
```

### Check if authenticated
```bash
if standx auth status | grep -q "Authenticated"; then
  echo "Logged in"
else
  echo "Not logged in"
fi
```

### Monitor price changes
```bash
while true; do
  PRICE=$(standx -o quiet market ticker BTC-USD)
  echo "$(date): BTC = $PRICE"
  sleep 60
done
```
