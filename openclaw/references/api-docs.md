# API Documentation

## Overview

StandX CLI provides a command-line interface to the StandX exchange API.

## Base URL

Configured via `standx config get base_url`

Default: `https://api.standx.com`

## Authentication

All trading endpoints require authentication via JWT token.

See [Authentication Details](authentication.md) for more information.

## Rate Limits

- Market data: 100 requests/minute
- Trading: 20 requests/minute
- Account: 60 requests/minute

## Endpoints

### Market Data

| Command | Description |
|---------|-------------|
| `standx market symbols` | List all trading pairs |
| `standx market ticker <symbol>` | Get current price |
| `standx market depth <symbol>` | Order book depth |
| `standx market kline <symbol>` | Candlestick data |
| `standx market funding <symbol>` | Funding rate history |

### Account

| Command | Description |
|---------|-------------|
| `standx account balances` | Get account balances |
| `standx account positions` | Get open positions |
| `standx account orders` | Get active orders |
| `standx account history` | Get order history |

### Trading

| Command | Description |
|---------|-------------|
| `standx order create` | Create a new order |
| `standx order cancel` | Cancel an order |
| `standx order cancel-all` | Cancel all orders |
| `standx trade history` | Get trade history |

## Error Codes

| Code | Description |
|------|-------------|
| 400 | Bad Request |
| 401 | Unauthorized |
| 403 | Forbidden |
| 404 | Not Found |
| 429 | Rate Limited |
| 500 | Internal Server Error |
