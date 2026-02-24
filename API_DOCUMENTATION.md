# StandX API Documentation

## Base URLs

- **REST API**: `https://perps.standx.com`
- **WebSocket**: `wss://perps.standx.com/ws-stream/v1`

## Authentication

### JWT Token

Visit [https://standx.com/user/session](https://standx.com/user/session) to generate:
- JWT Token (valid for 7 days)
- Ed25519 Private Key (Base58 encoded)

### Request Headers

All authenticated requests must include:

```
Authorization: Bearer <JWT_TOKEN>
x-request-sign-version: v1
x-request-id: <UUID>
x-request-timestamp: <UNIX_TIMESTAMP_MS>
x-request-signature: <BASE64_SIGNATURE>
```

### Request Signing

Sign the following message with Ed25519:

```
message = "v1,request_id,timestamp,payload"
```

Where `payload` is the request body (or empty string for GET requests).

## Public Endpoints

### Get Symbol Info

```http
GET /api/query_symbol_info
```

Returns information about all available trading pairs.

**Response:**
```json
[
  {
    "symbol": "BTC-USD",
    "base_asset": "BTC",
    "quote_asset": "DUSD",
    "base_decimals": 9,
    "price_tick_decimals": 2,
    "qty_tick_decimals": 4,
    "min_order_qty": "0.0001",
    "def_leverage": "10",
    "max_leverage": "40",
    "maker_fee": "0.0001",
    "taker_fee": "0.0004",
    "status": "trading"
  }
]
```

### Get Market Data

```http
GET /api/query_symbol_market?symbol=BTC-USD
```

Returns market data including funding rate.

**Response:**
```json
{
  "symbol": "BTC-USD",
  "mark_price": "63127.37",
  "index_price": "63126.67",
  "last_price": "63115.80",
  "volume_24h": 8755.733700000033,
  "high_price_24h": 66571.0,
  "low_price_24h": 62684.48,
  "funding_rate": "0.00001250",
  "next_funding_time": "2026-02-24T09:00:00Z"
}
```

### Get Order Book Depth

```http
GET /api/query_depth_book?symbol=BTC-USD&limit=10
```

Returns order book depth.

**Response:**
```json
{
  "asks": [
    ["63239", "3.6699"],
    ["63245", "0.20"]
  ],
  "bids": [
    ["63230", "1.0822"],
    ["63229", "0.0473"]
  ]
}
```

### Get Recent Trades

```http
GET /api/query_recent_trades?symbol=BTC-USD&limit=100
```

Returns recent trades.

**Response:**
```json
[
  {
    "is_buyer_taker": true,
    "price": "63102.97",
    "qty": "0.0320",
    "quote_qty": "2019.295040",
    "symbol": "BTC-USD",
    "time": "2026-02-24T08:06:52.515929Z"
  }
]
```

### Get Kline History

```http
GET /api/kline/history?symbol=BTC-USD&resolution=1h&from=1704067200&to=1706659200
```

Returns historical kline/candlestick data.

**Parameters:**
- `symbol`: Trading pair symbol
- `resolution`: Time resolution (1m, 5m, 15m, 1h, 4h, 1d)
- `from`: Start timestamp (Unix seconds)
- `to`: End timestamp (Unix seconds)

**Response:**
```json
[
  {
    "time": "2026-01-01T00:00:00Z",
    "open": "63000",
    "high": "63500",
    "low": "62800",
    "close": "63200",
    "volume": "100.5"
  }
]
```

### Get Funding Rates

```http
GET /api/query_funding_rates?symbol=BTC-USD&start_time=1700000000&end_time=1700000100
```

Returns funding rate history.

**Response:**
```json
[
  {
    "symbol": "BTC-USD",
    "funding_rate": "0.00001250",
    "next_funding_time": "2026-02-24T09:00:00Z"
  }
]
```

## Authenticated Endpoints

### Get Balance

```http
GET /api/query_balance
Authorization: Bearer <JWT>
```

Returns account balances.

**Response:**
```json
[
  {
    "asset": "DUSD",
    "available": "10000.00",
    "frozen": "500.00",
    "total": "10500.00"
  }
]
```

### Get Positions

```http
GET /api/query_positions?symbol=BTC-USD
Authorization: Bearer <JWT>
```

Returns position information.

**Response:**
```json
[
  {
    "symbol": "BTC-USD",
    "side": "Long",
    "quantity": "0.5",
    "entry_price": "63000",
    "mark_price": "63127.37",
    "liquidation_price": "50000",
    "margin": "3150",
    "leverage": "10",
    "unrealized_pnl": "63.68"
  }
]
```

### Get Open Orders

```http
GET /api/query_open_orders?symbol=BTC-USD
Authorization: Bearer <JWT>
```

Returns open orders.

**Response:**
```json
[
  {
    "id": "12345",
    "symbol": "BTC-USD",
    "side": "Buy",
    "type": "Limit",
    "quantity": "0.1",
    "filled_quantity": "0",
    "price": "62000",
    "status": "New",
    "created_at": "2026-02-24T08:00:00Z",
    "updated_at": "2026-02-24T08:00:00Z"
  }
]
```

### Create Order

```http
POST /api/new_order
Authorization: Bearer <JWT>
Content-Type: application/json
x-request-sign-version: v1
x-request-id: <UUID>
x-request-timestamp: <TIMESTAMP>
x-request-signature: <SIGNATURE>

{
  "symbol": "BTC-USD",
  "side": "buy",
  "order_type": "limit",
  "qty": "0.1",
  "price": "63000",
  "time_in_force": "GTC",
  "reduce_only": false,
  "sl_price": "62000",
  "tp_price": "65000"
}
```

Creates a new order.

**Request Fields:**
- `symbol`: Trading pair symbol
- `side`: `buy` or `sell`
- `order_type`: `limit` or `market`
- `qty`: Order quantity
- `price`: Order price (required for limit orders)
- `time_in_force`: `GTC`, `IOC`, or `FOK`
- `reduce_only`: Close position only
- `sl_price`: Stop-loss price (optional)
- `tp_price`: Take-profit price (optional)

**Response:**
```json
{
  "code": 0,
  "message": "Success",
  "request_id": "uuid"
}
```

### Cancel Order

```http
POST /api/cancel_order
Authorization: Bearer <JWT>
Content-Type: application/json
x-request-sign-version: v1
x-request-id: <UUID>
x-request-timestamp: <TIMESTAMP>
x-request-signature: <SIGNATURE>

{
  "symbol": "BTC-USD",
  "order_id": 12345
}
```

Cancels an order by ID.

### Cancel Multiple Orders

```http
POST /api/cancel_orders
Authorization: Bearer <JWT>
Content-Type: application/json
x-request-sign-version: v1
x-request-id: <UUID>
x-request-timestamp: <TIMESTAMP>
x-request-signature: <SIGNATURE>

{
  "symbol": "BTC-USD",
  "order_id_list": [12345, 12346]
}
```

Cancels multiple orders.

## WebSocket API

### Connection

Connect to `wss://perps.standx.com/ws-stream/v1`

### Authentication

Send authentication message immediately after connection:

```json
{
  "auth": {
    "token": "eyJ..."
  }
}
```

### Subscription

Subscribe to channels after successful authentication:

```json
{
  "subscribe": {
    "channel": "depth_book",
    "symbol": "BTC-USD"
  }
}
```

### Available Channels

#### depth_book

Order book updates.

**Message Format:**
```json
{
  "channel": "depth_book",
  "data": {
    "symbol": "BTC-USD",
    "bids": [["63230", "1.0822"], ["63229", "0.0473"]],
    "asks": [["63239", "3.6699"], ["63245", "0.20"]]
  },
  "seq": 12345
}
```

#### price

Price ticker updates.

**Message Format:**
```json
{
  "channel": "price",
  "data": {
    "symbol": "BTC-USD",
    "mark_price": "63127.37",
    "index_price": "63126.67",
    "last_price": "63115.80",
    "time": "2026-02-24T08:06:48.645735Z"
  }
}
```

#### position

Position updates (authenticated).

**Message Format:**
```json
{
  "channel": "position",
  "data": {
    "symbol": "BTC-USD",
    "side": "Long",
    "quantity": "0.5",
    "entry_price": "63000",
    "mark_price": "63127.37"
  }
}
```

#### balance

Balance updates (authenticated).

**Message Format:**
```json
{
  "channel": "balance",
  "data": {
    "token": "DUSD",
    "available": "10000.00",
    "frozen": "500.00"
  }
}
```

#### order

Order updates (authenticated).

**Message Format:**
```json
{
  "channel": "order",
  "data": {
    "id": "12345",
    "symbol": "BTC-USD",
    "status": "Filled",
    "filled_quantity": "0.1"
  }
}
```

## Error Codes

| Code | Description |
|------|-------------|
| 400 | Bad Request |
| 401 | Unauthorized - Invalid or missing JWT |
| 403 | Forbidden - Invalid signature |
| 404 | Not Found |
| 429 | Rate Limited |
| 500 | Internal Server Error |

## Rate Limits

- Public API: 100 requests per minute per IP
- Authenticated API: 200 requests per minute per account
- WebSocket: 1 connection per account

## Data Types

### Order Side
- `buy` - Buy/Long
- `sell` - Sell/Short

### Order Type
- `limit` - Limit order
- `market` - Market order

### Time in Force
- `GTC` - Good Till Cancel
- `IOC` - Immediate or Cancel
- `FOK` - Fill or Kill

### Order Status
- `New` - New order
- `PartiallyFilled` - Partially filled
- `Filled` - Completely filled
- `Canceled` - Canceled
- `Rejected` - Rejected
- `Expired` - Expired

### Position Side
- `Long` - Long position
- `Short` - Short position
