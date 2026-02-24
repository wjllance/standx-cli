# StandX CLI Development Plan

## Project Overview

A comprehensive Rust CLI tool for interacting with the StandX perpetual DEX API, featuring REST API access, WebSocket streaming, and secure authentication.

## Architecture

```
standx-cli/
├── src/
│   ├── main.rs              # CLI entry point with command routing
│   ├── cli.rs               # Clap argument definitions
│   ├── commands.rs          # Command handler implementations
│   ├── lib.rs               # Library module exports
│   ├── auth/                # Authentication module
│   │   ├── mod.rs           # Ed25519 signing implementation
│   │   └── credentials.rs   # Secure credential storage
│   ├── client/              # HTTP API client
│   │   ├── mod.rs           # Public API endpoints
│   │   ├── account.rs       # Authenticated account API
│   │   └── order.rs         # Order management API
│   ├── config.rs            # Configuration management
│   ├── error.rs             # Error types and handling
│   ├── models.rs            # Data models and serialization
│   ├── output.rs            # Output formatting (table/json/csv)
│   └── websocket.rs         # WebSocket streaming client
├── Cargo.toml
├── README.md
└── LICENSE
```

## Implementation Phases

### Phase 1: Project Scaffold ✅
- [x] Initialize Rust project with Cargo
- [x] Set up CI/CD workflow
- [x] Add basic README and LICENSE
- [x] Configure dependencies

### Phase 2: Public API Implementation ✅
- [x] HTTP client with reqwest
- [x] Symbol info endpoint
- [x] Market data endpoint
- [x] Price endpoint
- [x] Recent trades endpoint
- [x] Error handling module

### Phase 3: Authentication System ✅
- [x] Ed25519 signing with ed25519-dalek
- [x] JWT token management
- [x] Secure credential storage
- [x] Token expiration tracking
- [x] Auth CLI commands (login/logout/status)

### Phase 4: CLI Commands ✅
- [x] Config commands (init/set/get/show)
- [x] Market commands with output formatting
- [x] Table/JSON/CSV output support
- [x] Comprehensive error messages

### Phase 5: Account API ✅
- [x] Balance query endpoint
- [x] Positions query endpoint
- [x] Open orders endpoint
- [x] Order history endpoint
- [x] Account CLI commands

### Phase 6: Order API ✅
- [x] Create order endpoint
- [x] Cancel order endpoint
- [x] Batch cancel orders
- [x] Request signing for authenticated endpoints
- [x] Order CLI commands

### Phase 7: WebSocket Streaming ✅
- [x] WebSocket client with tokio-tungstenite
- [x] Auto-reconnect with exponential backoff
- [x] Heartbeat monitoring
- [x] JWT authentication
- [x] Channel subscription management
- [x] Stream CLI commands

## Key Technical Decisions

### Authentication
- **JWT Token**: Pre-generated from StandX website (7-day validity)
- **Ed25519 Signing**: Request signature for authenticated endpoints
- **Credential Storage**: XOR-encrypted file with restrictive permissions

### API Compatibility
- **Base URL**: `https://perps.standx.com`
- **WebSocket URL**: `wss://perps.standx.com/ws-stream/v1`
- **Field Types**: Flexible deserialization (string or number to string)
- **Field Names**: Matched to actual API response (e.g., `high_price_24h`)

### Error Handling
- Custom `Error` enum with `thiserror`
- HTTP status code preservation
- Descriptive error messages
- Proper error propagation with `?`

### Output Formatting
- **Table**: Human-readable with `tabled`
- **JSON**: Machine-readable with `serde_json`
- **CSV**: Spreadsheet-compatible with `csv`

## Testing Strategy

### Unit Tests
- Model serialization/deserialization
- Authentication logic
- Output formatting

### Integration Tests
- API endpoint mocking with `mockito`
- Error response handling

### Test Coverage
```
✅ 24 tests passing
├── auth::tests (4 tests)
├── client::tests (4 tests)
├── models::tests (5 tests)
├── config::tests (3 tests)
├── output::tests (2 tests)
├── client::account::tests (1 test)
├── client::order::tests (1 test)
└── websocket::tests (1 test)
```

## API Endpoint Mapping

### Public Endpoints
| Endpoint | CLI Command | Status |
|----------|-------------|--------|
| GET /api/query_symbol_info | `market symbols` | ✅ |
| GET /api/query_symbol_market | `market ticker` | ✅ |
| GET /api/query_symbol_price | (internal use) | ✅ |
| GET /api/query_depth_book | `market depth` | ✅ |
| GET /api/query_recent_trades | `market trades` | ✅ |
| GET /api/kline/history | `market kline` | ✅ |
| GET /api/query_funding_rates | `market funding` | ✅ |

### Authenticated Endpoints
| Endpoint | CLI Command | Status |
|----------|-------------|--------|
| GET /api/query_balance | `account balances` | ✅ |
| GET /api/query_positions | `account positions` | ✅ |
| GET /api/query_open_orders | `account orders` | ✅ |
| GET /api/query_order_history | `account history` | ✅ |
| POST /api/new_order | `order create` | ✅ |
| POST /api/cancel_order | `order cancel` | ✅ |
| POST /api/cancel_orders | `order cancel-all` | ✅ |

### WebSocket Channels
| Channel | CLI Command | Type |
|---------|-------------|------|
| depth_book | `stream depth` | Public |
| price | `stream ticker` | Public |
| position | `stream account` | Private |
| balance | `stream account` | Private |
| order | `stream account` | Private |

## Dependencies

### Core
- `tokio` - Async runtime
- `clap` - CLI argument parsing
- `reqwest` - HTTP client
- `serde` - Serialization

### Cryptography
- `ed25519-dalek` - Ed25519 signing
- `bs58` - Base58 encoding

### WebSocket
- `tokio-tungstenite` - WebSocket client
- `futures-util` - Async utilities

### Output
- `tabled` - Table formatting
- `csv` - CSV output

### Utilities
- `chrono` - Date/time handling
- `dirs` - System directories
- `rpassword` - Secure password input

## Future Enhancements

### Potential Features
- [ ] Order modification
- [ ] Batch order creation
- [ ] Position closing
- [ ] Margin mode switching
- [ ] Transfer between accounts
- [ ] Historical data export
- [ ] Trading bot integration
- [ ] GUI dashboard

### Performance Optimizations
- [ ] Connection pooling
- [ ] Request caching
- [ ] Parallel API calls
- [ ] Compression support

## References

- [StandX Website](https://standx.com)
- [ritmex-bot](https://github.com/discountry/ritmex-bot) - Reference implementation
- [standx_mm_bot](https://github.com/NA-DEGEN-GIRL/standx_mm_bot) - Market maker bot
