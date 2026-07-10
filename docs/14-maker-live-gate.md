# Maker live gate

`STANDX_ENABLE_LIVE_MAKER=1` is an intentional lock, not an operational
toggle. It must remain unset until every gate below has evidence attached to
the release/change record.

## Required engineering evidence

- All maker risk PRs are merged in dependency order and CI is green.
- `cargo test -p standx-sdk --offline`, `cargo test -p standx-cli --lib --offline`, and strict workspace Clippy pass on the merge commit.
- Replay scenarios cover: crossed touch, stale/crossed market data, volatility halt, position-limit pressure, delayed order visibility, stream loss, and inventory exit.
- No telemetry, logs, PR body, or alert output contains credentials or webhook secrets.

## Paper and connectivity evidence

- Paper mode completes a recorded multi-hour session without panic or invariant failure.
- Order-response authentication succeeds; a forced disconnect produces fail-safe shutdown and maker-order cleanup.
- Fill ledger records only `sxmk-` correlated venue fills and does not duplicate trade IDs.
- Inventory-exit configuration remains disabled unless a supervised test has approved its exact threshold and chunk.

## Supervised canary

- A named operator is present with venue access and a documented emergency cancel procedure.
- Start with one symbol, minimum valid size, one level, and a max position no larger than one exit chunk.
- Observe startup, order response, fills, cancellations, alerts, and shutdown; retain timestamps and order/trade IDs.
- Stop immediately on residual maker orders, uncorrelated fills, stream disconnect, unexpected position change, failed cleanup, or breached risk limit.

## Unlock decision

Only a release owner may enable the environment variable after the preceding
evidence is reviewed. The first canary does not grant permanent approval: any
strategy, venue API, credential, or risk-control change re-locks the gate and
requires a new canary record.

## Execution records

- [2026-07-10 maker paper long run and controlled disconnect](evidence/maker-live-gate-2026-07-10.md)
