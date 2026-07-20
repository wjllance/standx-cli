# StandX↔Hyperliquid lag-recorder tool delivery evidence — 2026-07-20

## Decision

- Status: `tool_delivered_pending_field_data`
- This is a read-only diagnostic tool, not a strategy change. It performs no
  authentication, places no orders, and shares no code path with the maker
  trading runtime. It is therefore **not** subject to the live-gate / canary
  process (no strategy, risk, or exchange-command path changed).
- No lag measurement is claimed yet. The tool must be run for a long,
  volatility-covering window **on the maker's deployment host/region** before
  any number is quoted (see Honest limitations).
- PR: [#319](https://github.com/wjllance/standx-cli/pull/319), branch
  `lag-recorder-standx-hyperliquid`, commit
  `c1cef19db5a3e35a833b1365dd8eb36f9a74f059`.

## Motivation

The HYPE maker loss diagnosis converged on jump-kill toxicity: 74% of toxic
fills have no local order-book precursor within a 3s cycle, and the loss tail
(89% of mo300 loss mass) is instantaneous cross-throughs. The working
hypothesis is that these are driven by an external leading market — StandX's
mark price is an aggregate and lags. This tool measures that lag so the
external-price-guided-quoting route can be decided on evidence rather than
assumption:

- lag `< ~0.3s` → no exploitable window (our own cancel takes 0.3–0.5s) → the
  route is dead, no development;
- lag `1–3s` → a defensible window exists → candidate for roadmap Stage 4
  (fair-price / order-flow), with the external mark replacing microprice as the
  leading signal.

## What was delivered

- New subcommand `standx lag-recorder`
  (`crates/standx-cli/src/commands/lag_recorder.rs`):
  - StandX side reuses the public `price` + `depth_book` WebSocket feed via the
    existing SDK client (`StandXWebSocket::without_auth`) — no credentials.
  - Hyperliquid side is a minimal WebSocket client on the public
    `activeAssetCtx` channel (`markPx` / `midPx` / `oraclePx`), with a 30s
    application-level ping and reconnect-on-drop.
  - Every update is timestamped **at receipt on one process-wide monotonic
    clock** (comparable across the two producer tasks) plus a UTC correlation
    stamp. Output is one NDJSON line per update with a stable schema
    (`source`, `local_recv_ms`, `local_recv_utc`, `mark`, `mid`, `index`,
    `last`, `best_bid`, `best_ask`, `server_time`, `seq`), appended through a
    `BufWriter` flushed on a timer.
  - Both SIGINT and SIGTERM drive a graceful flush-and-exit; each feed
    self-heals on disconnect.
- Offline analyzer `scripts/lag_analysis.py` (stdlib only, style aligned with
  `scripts/maker_markout_ab.py`): two independent lag estimates —
  cross-correlation of resampled price increments, and event-response
  (Hyperliquid jump → StandX follow-time to cover a fraction of the move).
- Wiring: `cli.rs` (`Commands::LagRecorder`), `main.rs` (dispatch +
  `command_name` + dry-run matches), `commands/mod.rs`; `tokio-tungstenite`
  added to the CLI crate (`Cargo.toml` / `Cargo.lock`, one new dependency edge).

## Offline verification

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: clean.
- `cargo test --workspace --offline`: all green — `standx-cli` lib 185
  (including 4 new `lag_recorder` unit tests: `derive_hl_coin`,
  `activeAssetCtx` parse, other-channel ignore, stable-schema serialization),
  `standx-maker` 157, `standx-sdk` 75, plus integration/unit/e2e/doc tests;
  the two credential-dependent e2e tests remained intentionally ignored.
- `python3 -m py_compile scripts/lag_analysis.py` and
  `scripts/openobserve_dashboard.py`: both ok.

## Functional smoke (plumbing, not a measurement)

- Live run against production public feeds, symbol HYPE-USD, `--status-secs 5`,
  output to a scratch NDJSON file; no credentials, no orders.
- Both sources recorded: `standx=67` records, `hyperliquid=27` records over a
  ~25s window; StandX `price` and `depth_book` lines carried the expected
  disjoint field sets (mark/index/last vs best_bid/best_ask).
- `kill -TERM` produced `shutdown signal received, flushing…` →
  `stopped. standx_records=67 hyperliquid_records=27`; all 94 NDJSON lines
  parsed as valid JSON (no truncated tail).
- The analyzer, run on this short/flat window, correctly reported
  "insufficient overlap for cross-correlation" and "no measurable follow
  events" rather than fabricating a lag — the desired fail-honest behavior.
- The `standx-hl` status figure shown during the smoke (~+9bps) is a **static
  price-level basis, not a lag**; it must not be read as a timing result.

## Honest limitations (documented in code and analyzer output)

- The common local clock carries a **fixed differential-network-latency
  offset**: the RTT from the recording host to StandX vs to Hyperliquid
  differs. The variable part (event-response spread, correlation shape) is
  robust; the absolute lag is biased by that differential. The recorder must
  run from the **same host/region as the maker** for the number to be
  representative.
- Resolution floor: Hyperliquid `activeAssetCtx` (~0.5s/block) and the StandX
  update cadence bound the smallest trustworthy lag; sub-0.5s point estimates
  are "below resolution", not zero.
- Single window, single symbol; the result is HYPE-specific and must be
  re-recorded per symbol.

## Next step (not in this delivery)

Run `standx lag-recorder` on the deployment host over a long window covering
calm and volatile regimes, then `python3 scripts/lag_analysis.py`. Decide the
external-price route by the thresholds above, jointly with the widen-spread A/B
verdict and the measured SIP-5A $/Maker-Hour. This tool only supplies the
number; it changes no quoting behavior.
