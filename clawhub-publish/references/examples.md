# Command Examples

## Market Data Examples

### Get BTC Price

```bash
standx market ticker BTC-USD
```

### Get Order Book (Top 5)

```bash
standx market depth BTC-USD --limit 5
```

### Get 1-Hour Candles (Last 24h)

```bash
standx market kline BTC-USD -r 60 --from 1d
```

### Get Daily Candles (Last 30 days)

```bash
standx market kline BTC-USD -r 1D --from 30d
```

## Trading Examples

### Limit Buy Order

```bash
standx order create BTC-USD buy limit --qty 0.01 --price 60000
```

### Market Sell Order

```bash
standx order create BTC-USD sell market --qty 0.01
```

### Cancel Specific Order

```bash
standx order cancel BTC-USD --order-id 123456
```

### Cancel All Orders

```bash
standx order cancel-all BTC-USD
```

## Account Examples

### Check Balances

```bash
standx account balances
```

### View Positions

```bash
standx account positions
```

### View Order History

```bash
standx account history --limit 50
```

## Leverage Examples

### Check Current Leverage

```bash
standx leverage get BTC-USD
```

### Set 10x Leverage

```bash
standx leverage set BTC-USD 10
```

### Check Margin Mode

```bash
standx margin mode BTC-USD
```

### Set Isolated Margin

```bash
standx margin mode BTC-USD --set isolated
```

## Streaming Examples

### Price Stream

```bash
standx stream price BTC-USD
```

### Order Book Stream

```bash
standx stream depth BTC-USD --levels 10
```

### Trade Stream

```bash
standx stream trade BTC-USD
```

## Output Format Examples

### JSON Output

```bash
standx -o json market ticker BTC-USD
```

### CSV Export

```bash
standx -o csv market symbols > symbols.csv
```

### Quiet Mode (Scripting)

```bash
PRICE=$(standx -o quiet market ticker BTC-USD)
echo "Current BTC price: $PRICE"
```
