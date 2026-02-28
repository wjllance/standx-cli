# GitHub Issues for Test Coverage Improvement

## Issue #67: [Test] Telemetry module comprehensive tests

**Labels**: `testing`, `telemetry`, `phase-3`, `help wanted`

**Description**:
The telemetry module (`src/telemetry.rs`) currently has no test coverage. We need to add unit tests to ensure the telemetry collection works correctly.

**Test Cases**:
- [ ] `test_telemetry_disabled_via_env` - Verify telemetry is disabled when `STANDX_TELEMETRY=0`
- [ ] `test_telemetry_enabled_by_default` - Verify telemetry is enabled by default
- [ ] `test_telemetry_event_creation` - Verify TelemetryEvent struct creation
- [ ] `test_telemetry_command_started_event` - Verify CommandStarted event recording
- [ ] `test_telemetry_command_completed_event` - Verify CommandCompleted event recording
- [ ] `test_telemetry_error_event` - Verify Error event recording
- [ ] `test_telemetry_file_write` - Verify events are written to file (use tempdir)
- [ ] `test_telemetry_session_id_persistence` - Verify session ID remains consistent

**Implementation Notes**:
- Use `tempfile` crate for temporary directory in tests
- Use `EnvGuard` pattern (see `src/auth/credentials.rs`) for environment variable isolation
- Place tests inline in `src/telemetry.rs` under `#[cfg(test)]`

**Estimated Effort**: 0.5 day
**Priority**: High

---

## Issue #68: [Test] Error module display and conversion tests

**Labels**: `testing`, `error`, `phase-3`, `good first issue`

**Description**:
The error module (`src/error.rs`) defines all error types but lacks tests for error messages and conversions.

**Test Cases**:
- [ ] `test_error_display_api` - Verify Api error displays code and message
- [ ] `test_error_display_auth_required` - Verify AuthRequired error shows message and resolution
- [ ] `test_error_display_config` - Verify Config error displays correctly
- [ ] `test_error_display_network` - Verify Network error displays correctly
- [ ] `test_error_from_serde_json` - Verify conversion from serde_json::Error
- [ ] `test_error_from_reqwest` - Verify conversion from reqwest::Error
- [ ] `test_error_from_io` - Verify conversion from std::io::Error
- [ ] `test_result_type_alias` - Verify the Result<T> type alias works correctly

**Implementation Notes**:
- Place tests inline in `src/error.rs` under `#[cfg(test)]`
- Test both the `Display` trait and error conversions

**Estimated Effort**: 0.5 day
**Priority**: High

---

## Issue #69: [Test] WebSocket module state and message tests

**Labels**: `testing`, `websocket`, `phase-3`

**Description**:
The WebSocket module (`src/websocket.rs`) has only one test (`test_ws_state`). We need comprehensive tests for state management and message parsing.

**Test Cases**:
- [ ] `test_ws_state_transitions` - Test Disconnected -> Connecting -> Connected flow
- [ ] `test_ws_state_reconnecting` - Test Connected -> Reconnecting -> Connected flow
- [ ] `test_ws_message_parse_price` - Test PriceData message parsing from JSON
- [ ] `test_ws_message_parse_depth` - Test OrderBook message parsing from JSON
- [ ] `test_ws_message_parse_trade` - Test Trade message parsing from JSON
- [ ] `test_ws_message_parse_position` - Test Position message parsing from JSON
- [ ] `test_ws_message_parse_error` - Test error message handling
- [ ] `test_ws_subscription_add` - Test adding subscriptions
- [ ] `test_ws_subscription_remove` - Test removing subscriptions
- [ ] `test_ws_reconnect_attempts_increment` - Test reconnect counter increments

**Implementation Notes**:
- Message parsing tests can use JSON fixtures
- State tests don't require actual WebSocket connection
- Consider using `tokio::test` for async tests

**Estimated Effort**: 1 day
**Priority**: High

---

## Issue #70: [Test] Client/Account API comprehensive tests

**Labels**: `testing`, `client`, `account`, `phase-4`

**Description**:
The account client (`src/client/account.rs`) has only one test. Add comprehensive tests for all account-related API calls.

**Test Cases**:
- [ ] `test_get_balance_success` - Mock successful balance response
- [ ] `test_get_balance_with_assets` - Test balance with multiple assets
- [ ] `test_get_positions_success` - Mock successful positions response
- [ ] `test_get_positions_empty` - Test empty positions list
- [ ] `test_get_positions_with_multiple` - Test multiple positions
- [ ] `test_get_orders_success` - Mock open orders response
- [ ] `test_get_orders_empty` - Test empty orders list
- [ ] `test_get_order_history_success` - Mock order history response
- [ ] `test_get_order_history_with_pagination` - Test pagination params

**Implementation Notes**:
- Use `mockito` for HTTP mocking (see existing tests in `src/client/tests.rs`)
- Test both success and empty response cases
- Verify correct parsing of response JSON

**Estimated Effort**: 0.5 day
**Priority**: Medium

---

## Issue #71: [Test] Client/Order API comprehensive tests

**Labels**: `testing`, `client`, `order`, `phase-4`

**Description**:
The order client (`src/client/order.rs`) has only one test. Add tests for various order types and scenarios.

**Test Cases**:
- [ ] `test_create_order_limit` - Test limit order creation
- [ ] `test_create_order_market` - Test market order creation
- [ ] `test_create_order_with_stop_loss` - Test order with stop loss
- [ ] `test_create_order_with_take_profit` - Test order with take profit
- [ ] `test_create_order_reduce_only` - Test reduce-only order
- [ ] `test_create_order_ioc` - Test IOC time-in-force
- [ ] `test_cancel_order_success` - Test cancel specific order
- [ ] `test_cancel_all_orders` - Test cancel all orders for symbol
- [ ] `test_get_trades_success` - Test trade history retrieval

**Implementation Notes**:
- Use `mockito` for HTTP mocking
- Test different order parameters
- Verify request body structure

**Estimated Effort**: 0.5 day
**Priority**: Medium

---

## Issue #72: [Test] CLI argument parsing tests

**Labels**: `testing`, `cli`, `phase-4`, `good first issue`

**Description**:
The CLI module (`src/cli.rs`) uses clap for argument parsing but has no tests. Add tests to verify CLI structure.

**Test Cases**:
- [ ] `test_cli_parse_version` - Test --version flag
- [ ] `test_cli_parse_help` - Test --help flag
- [ ] `test_cli_parse_market_command` - Test market subcommand parsing
- [ ] `test_cli_parse_account_command` - Test account subcommand parsing
- [ ] `test_cli_parse_output_format` - Test -o/--output flag
- [ ] `test_cli_parse_openclaw_flag` - Test --openclaw flag
- [ ] `test_cli_parse_dry_run_flag` - Test --dry-run flag
- [ ] `test_cli_parse_verbose_flag` - Test -v/--verbose flag

**Implementation Notes**:
- Use `clap`'s built-in test support or `try_parse_from`
- Test both valid and invalid argument combinations
- Place tests inline in `src/cli.rs`

**Estimated Effort**: 0.5 day
**Priority**: Medium

---

## Summary

| Issue | Module | Test Count | Effort | Priority |
|-------|--------|-----------|--------|----------|
| #67 | Telemetry | 8 | 0.5d | High |
| #68 | Error | 8 | 0.5d | High |
| #69 | WebSocket | 10 | 1d | High |
| #70 | Client/Account | 9 | 0.5d | Medium |
| #71 | Client/Order | 9 | 0.5d | Medium |
| #72 | CLI | 8 | 0.5d | Medium |
| **Total** | | **52** | **3.5d** | |

---

*Document created: 2026-02-28*
