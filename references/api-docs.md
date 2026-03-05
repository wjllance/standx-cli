# StandX CLI API Documentation

## Base URL

```
https://perps.standx.com
```

## Public Endpoints

### GET /api/query_symbol_info
List all trading pairs.

### GET /api/query_symbol_market
Get ticker for a symbol.

**Parameters:**
- `symbol` (required): Trading pair, e.g., "BTC-USD"

### GET /api/query_depth_book
Get order book depth.

**Parameters:**
- `symbol` (required): Trading pair
- `limit` (optional): Depth levels, default 10

### GET /api/kline/history
Get K-line (candlestick) data.

**Parameters:**
- `symbol` (required): Trading pair
- `resolution` (required): Time period (1, 5, 15, 30, 60, 240, 720, 1D, 1W, 1M)
- `from` (required): Start timestamp (Unix seconds)
- `to` (required): End timestamp (Unix seconds)

**Response format:**
```json
{
  "s": "ok",
  "t": [timestamp1, timestamp2, ...],
  "o": [open1, open2, ...],
  "h": [high1, high2, ...],
  "l": [low1, low2, ...],
  "c": [close1, close2, ...],
  "v": [volume1, volume2, ...]
}
```

### GET /api/query_funding_rates
Get funding rate history.

**Parameters:**
- `symbol` (required): Trading pair
- `start_time` (required): Start timestamp (Unix milliseconds)
- `end_time` (required): End timestamp (Unix milliseconds)

## Authenticated Endpoints

All authenticated endpoints require:
- `Authorization: Bearer <JWT_TOKEN>` header
- Ed25519 request signature for trading operations

### GET /api/query_balance
Get account balances.

### GET /api/query_positions
Get positions.

**Parameters:**
- `symbol` (optional): Filter by trading pair

### GET /api/query_open_orders
Get open orders.

### GET /api/query_orders
Get order history.

**Parameters:**
- `status` (optional): "filled" for completed orders
- `symbol` (optional): Filter by trading pair
- `limit` (optional): Number of results

### POST /api/order/create
Create a new order.

**Request body:**
```json
{
  "symbol": "BTC-USD",
  "side": "buy",
  "order_type": "limit",
  "qty": "0.01",
  "price": "60000",
  "time_in_force": "gtc"
}
```

### POST /api/order/cancel
Cancel an order.

### GET /api/query_trades
Get trade history.

### GET /api/query_position_config
Get position configuration (leverage, margin mode).

### POST /api/change_leverage
Change leverage for a symbol.

### POST /api/change_margin_mode
Change margin mode (cross/isolated).

## WebSocket

**URL:** `wss://perps.standx.com/ws-stream/v1`

### Public Channels
- `price` - Price ticker
- `depth_book` - Order book depth
- `public_trade` - Public trades

### User Channels (requires auth)
- `order` - Order updates
- `position` - Position updates
- `balance` - Balance updates
- `fill` - Fill/trade updates

### Authentication Message
```json
{
  "auth": {
    "token": "Bearer <JWT_TOKEN>",
    "streams": [
      {"channel": "order"},
      {"channel": "position"}
    ]
  }
}
```
