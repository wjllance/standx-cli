# Maker live gate

`STANDX_ENABLE_LIVE_MAKER=1` is an intentional release-owner gate, not an
automatic start command. It was unlocked on 2026-07-10 after the paper run,
engineering checks, and supervised production canary were recorded below.
The flag only permits a future `--live` invocation; it does not start the
maker by itself.

## Required engineering evidence

- All maker risk PRs are merged in dependency order and CI is green.
- `cargo test -p standx-sdk --offline`, `cargo test -p standx-cli --lib --offline`, and strict workspace Clippy pass on the merge commit.
- Replay scenarios cover: crossed touch, stale/crossed market data, volatility halt, position-limit pressure, delayed order visibility, stream loss, and inventory exit.
- No telemetry, logs, PR body, or alert output contains credentials or webhook secrets.

## Paper and connectivity evidence

- Paper mode completes a recorded multi-hour session without panic or invariant failure.
- Order-response and authenticated account-stream authentication succeed; a forced account-stream disconnect freezes placements, cleans maker orders, and only resumes after authentication plus an empty-book/position snapshot.
- Fill ledger records only venue fills whose client-order ID carries the current run tag, enforces the session time boundary, and does not duplicate trade IDs. Historical `sxmk-` trades are ledger-sync evidence, not current-run fills.
- Existing inventory at or below `max_position` is adopted at the startup mark with maker-session PnL zeroed; inventory above the limit rejects startup after maker-order cleanup.
- A venue position change that cannot yet be explained by current-run order callbacks freezes placements immediately. WS order updates and REST snapshots are reconciled for at most three seconds; persistent mismatch triggers fail-safe cleanup and shutdown.
- Account-order cumulative fills and REST trades are tested in both arrival orders and never double-count fills, expected position, or maker-session PnL.
- Every accepted current-run fill atomically synchronizes ledger position and session telemetry position; an internal mismatch beyond half a quantity tick fails closed before further live placement or risk evaluation.
- Position jumps, account-stream state changes, reconciliation, volatility breaker, inventory exit, residual cleanup, and final fail-safe emit `risk_notification` telemetry; critical shutdown/cleanup delivery is awaited with a timeout.
- Inventory-exit configuration remains disabled unless a supervised test has approved its exact threshold and chunk; the recorded XAG-USD test approves only `max_position=0.8`, trigger `25%`, and chunk `0.2`.

## Supervised canary

- A named operator is present with venue access and a documented emergency cancel procedure.
- Start with one symbol, minimum valid size, one level, and a max position no larger than one exit chunk.
- For a changed WS command path, retain one correlation chain containing the
  create `request_id` and accepted response, the REST-visible venue order ID
  and client-order ID, the cancel `request_id` and accepted response, REST
  absence, and a final position equal to the flat preflight baseline.
- Observe startup, order response, fills, cancellations, alerts, and shutdown;
  retain timestamps and order/trade IDs. The hidden `ws-command-canary` emits
  these checks as `action=ws_command_canary` JSON events in every output mode.
- Stop immediately on residual maker orders, uncorrelated fills, stream disconnect, unexpected position change, failed cleanup, or breached risk limit.

## Unlock decision

Only a release owner may enable the environment variable after the preceding
evidence is reviewed. The explicit unlock approval was recorded on
2026-07-10. Any strategy, venue API, credential, or risk-control change
re-locks the gate and requires a new canary record.

## Execution records

- [2026-07-14 WS order-command controlled canary](evidence/maker-ws-order-command-canary-2026-07-14.md)
- [2026-07-10 maker paper long run, controlled disconnect, and supervised production canary](evidence/maker-live-gate-2026-07-10.md)
