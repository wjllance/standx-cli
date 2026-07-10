# Maker live gate record — 2026-07-10

## Decision

**PAPER EVIDENCE RECORDED — LIVE GATE REMAINS LOCKED.**

This record separates repeatable local evidence from production venue
evidence. Passing the controlled disconnect harness does not prove production
order-response authentication, production cancellation, or a supervised live
canary.

## Scope and build identity

- Repository: `wjllance/standx-cli`
- Runtime commit: `5dd0c10` (`main`, equal to `origin/main` at start)
- Symbol/mode: `BTC-USD`, paper
- Started: `2026-07-10T07:05:35Z`
- Strategy: `examples/maker.toml`, with explicit
  `--inventory-exit-pct 0 --inventory-exit-qty 0 --interval 5`
- Output: JSON lifecycle/action/cycle records plus stderr feed transitions
- Raw run log: [`logs/maker-paper-20260710T0705Z.log`](logs/maker-paper-20260710T0705Z.log)

## Paper long-run result

| Item | Recorded result |
|---|---|
| Window | `2026-07-10T07:05:35Z` to SIGINT at `2026-07-10T09:05:35Z` (2:00:00); last cycle summary at `09:05:33Z` |
| Cycle evidence | 1,437 `cycle_summary` records; cycles `0..1437` with one missing summary for cycle 11 after a transient request error |
| Two-sided uptime | 100.0% in the final summary |
| Fills | 9 paper-simulated fills (5 buy, 4 sell); final simulated position `+0.0010` |
| PnL | `+0.15882` mark-to-market at the final recorded mark; average capture `-2.05bps` |
| Volatility breaker | 0 halted cycles; no `HALT` action. `vol_bps` remained absent because the breaker never halted. |
| Quote churn | 227 cancellations, all `mark_moved`; no stale/crossed-order cancellation reason appeared. |
| Feed / recovery | Initial REST fallback then WebSocket live. One public price request failed at cycle 11 (`1/3`); the next logged cycle recovered through REST fallback and returned to WebSocket. It did not reach fail-safe. |
| Panic / invariant scan | No `panic`, `invariant`, `fail-safe`, `HALT`, or repeated cycle-failure record. |

Raw artifact: [`logs/maker-paper-20260710T0705Z.log`](logs/maker-paper-20260710T0705Z.log)
(`SHA-256 3e911860b50accba3b790f4eca52eac6cdb1386598b71e0b8b4d67bd66d87350`).

### Exit evidence and caveat

The two-hour run was sent Ctrl+C at the scheduled end and the paper process was
confirmed absent afterward. Its raw file ends at the final cycle summary but
does **not** include `lifecycle: stopped`: the process was launched through
`tee`, so the terminal signal ended the pipeline before that final JSON line
was persisted. This is a log-capture defect, not evidence of a clean long-run
shutdown, and must not be silently treated as one.

An immediately-following, no-pipe paper exit control did capture the expected
normal shutdown lifecycle: 35 cycles, 100% uptime, zero fills, and
`maker stopped (Ctrl+C)` at `2026-07-10T09:08:10Z`.
Its artifact is [`logs/maker-paper-clean-exit-20260710.log`](logs/maker-paper-clean-exit-20260710.log)
(`SHA-256 315e10db92b87b0b8dbffd91395e1737d2c8cc537217dcfb92d1d2a149a8d2dd`).

Result: **PASS for the recorded two-hour paper session and separately verified
normal paper exit path; caveat recorded for the long-run lifecycle capture.**

## Controlled disconnect and cleanup

The controlled test uses only loopback mock servers and a fake JWT. No
production order endpoint is contacted and no real order is submitted.

1. The mock order-response WebSocket accepts authentication and then closes.
   The SDK marks the stream unhealthy within the one-second assertion window.
2. A disconnected response receiver makes the maker fail closed with
   `order-response stream disconnected; refusing further live orders`.
3. Cleanup sees maker order `42` (`sxmk-controlled-buy`) and manual order `99`,
   sends a cancellation containing only order `42`, and verifies that no
   maker-owned order remains while manual order `99` is preserved.

Evidence:

- [`logs/maker-controlled-disconnect-ws-20260710.log`](logs/maker-controlled-disconnect-ws-20260710.log)
- [`logs/maker-controlled-disconnect-cleanup-20260710.log`](logs/maker-controlled-disconnect-cleanup-20260710.log)

Result: **PASS (local controlled harness only).** Production authentication and
cleanup evidence remains open.

## Engineering checks

| Check | Result | Evidence |
|---|---:|---|
| `cargo test -p standx-sdk --offline` | PASS, 52 unit + 1 doc | [`maker-gate-sdk-tests-20260710.log`](logs/maker-gate-sdk-tests-20260710.log) |
| `cargo test -p standx-cli --lib --offline` | PASS, 43 unit | [`maker-gate-cli-lib-tests-20260710.log`](logs/maker-gate-cli-lib-tests-20260710.log) |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS | [`maker-gate-clippy-20260710.log`](logs/maker-gate-clippy-20260710.log) |

These checks were run in the evidence worktree after adding the controlled
disconnect/cleanup regression. They are not represented as CI results on the
runtime commit.

## Remaining gate items

- Authenticate to the real production order-response stream under a named,
  supervised operator and retain the non-secret evidence.
- Force a production disconnect only under the documented canary procedure,
  verify maker-order cleanup against the venue, and retain order IDs/timestamps.
- Complete the supervised minimum-size live canary and release-owner review.

For a future paper run, capture stdout/stderr without a signal-sensitive pipe
so that the long-run's own `lifecycle: stopped` line is retained.

Until those items are reviewed, `STANDX_ENABLE_LIVE_MAKER` must remain unset.
