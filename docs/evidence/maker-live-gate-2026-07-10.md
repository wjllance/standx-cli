# Maker live gate record — 2026-07-10

## Decision

**PAPER EVIDENCE AND SUPERVISED PRODUCTION CANARY RECORDED — LIVE GATE UNLOCKED BY EXPLICIT RELEASE-OWNER APPROVAL.**

This record separates repeatable local evidence from production venue
evidence. The supervised canary below proves production authentication,
real-order placement, fail-safe shutdown, and maker-only cleanup for an empty
manual-order scope. The gate is now unlocked, but no continuous maker process
was started by this approval.

- Unlock approval: explicit user instruction received on `2026-07-10`.
- Persistent flag: `STANDX_ENABLE_LIVE_MAKER=1` added to the operator's
  `~/.zshrc`; this only permits a future `--live` invocation.

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

Result: **PASS (local controlled harness only).** Production evidence is
recorded separately in the supervised canary section below.

## Supervised production canary — BTC-USD

- Operator authorization: explicit approval in the task to run a production
  controlled disconnect and maker cleanup exercise without continuous live
  making.
- Credentials: loaded from `STANDX_JWT` and `STANDX_PRIVATE_KEY`; values were
  not logged. `STANDX_ENABLE_LIVE_MAKER=1` was scoped to the canary process only.
- Pre-check: production `BTC-USD` open orders `[]`; production position `[]`.
- Parameters: one level, size `0.001`, max position `0.001`, interval `5s`,
  `--controlled-disconnect-after 15`; no inventory exit and no webhook.
- Window: started `2026-07-10T09:34:17Z`; fail-safe stopped at
  `2026-07-10T09:34:37Z` after three cycles.
- Real orders: two initial maker placements (buy/sell), zero fills, zero
  position change.
- Fault: the authenticated production order-response stream was intentionally
  closed by the CLI's supervised fault-injection hook after 15 seconds.
- Fail-safe: `order-response stream is unhealthy; refusing further live orders`.
- Cleanup: venue query after shutdown returned `[]` maker orders; the process
  emitted `All maker-owned BTC-USD orders cancelled`.
- Post-check: production orders `[]`, production position `[]`.

Artifacts:

- [`logs/maker-production-controlled-disconnect-20260710.log`](logs/maker-production-controlled-disconnect-20260710.log)
  (`SHA-256 53bc8d8fef112e351d8e5fa890edbfed76087e34e3f990f1cb046306e9e4633e`)
- [`logs/maker-production-post-check-20260710.log`](logs/maker-production-post-check-20260710.log)
  (`SHA-256 2fa9fe3f3ea80a96ee9f1dfebe1da33aeb89a25707c4b150e2a91afefa0e6363`)

The outer zsh wrapper attempted to assign the reserved `status` variable after
the maker process exited; that wrapper-reporting error did not interrupt the
canary. The maker log contains the stopped lifecycle record and the post-check
completed successfully.

Result: **PASS for the supervised production canary.** The fault was injected
locally after production stream authentication; a venue/network-originated
disconnect remains a separate failure mode. No manual orders were present in
this canary, so preservation of foreign orders continues to rely on the local
mock proof above.

## Supervised production active inventory-exit test — XAG-USD

- Operator authorization: explicit approval to market-buy `0.2 XAG` and run a
  reduce-only inventory-exit test.
- Pre-check: production `XAG-USD` open orders `[]`; production position `[]`.
- Seed position: market buy `0.2 XAG`, order
  `8679b60a-c3cb-47aa-b45f-e1381cb0982f`, filled by the venue before the maker
  was started.
- Exact test tuple: `max_position=0.8`, `inventory_exit_pct=25`, and
  `inventory_exit_qty=0.2`; the trigger is therefore `0.2 XAG`.
- Saved operational profile: [`../../examples/maker-xag-100u.toml`](../../examples/maker-xag-100u.toml).
- Exit: on cycle 0 the maker cleared its book and submitted a `reduce_only`
  market sell. Venue history confirms `sxmk-exit-22b6d12a-349b-4c2f-8429-15a5502cb2f5`,
  `0.200 XAG`, fully filled at `59.48`.
- Cleanup: a controlled order-response disconnect then stopped the process;
  it reported all maker-owned `XAG-USD` orders cancelled.
- Post-check: production open orders `[]`; production position `[]`.

Artifacts:

- [`logs/maker-xag-inventory-exit-entry-20260710.log`](logs/maker-xag-inventory-exit-entry-20260710.log)
  (`SHA-256 35c521c1ba3a5aea974041a78fb66bd52c2cb955342990fd268ca2c78d7dab76`)
- [`logs/maker-xag-inventory-exit-live-20260710.log`](logs/maker-xag-inventory-exit-live-20260710.log)
  (`SHA-256 331dfa2e0fcfb4fccc35ee02e595e28a9ee5bc8236a3b5f019cd7b2d0e9d0992`)
- [`logs/maker-xag-inventory-exit-post-check-20260710.log`](logs/maker-xag-inventory-exit-post-check-20260710.log)
  (`SHA-256 b63378ddacc91e2529ef8ed74c6154c64df1eb6782eacd7ad463e6b166e51805`)

Result: **PASS for the exact active-exit tuple above.** This does not approve a
different threshold or chunk without another supervised test. The maker's
reported `PnL +11.89` during this run is not realized PnL: the seed entry was
an external/manual order and is deliberately absent from the maker-correlated
fill ledger. Do not run the maker against pre-existing manual inventory or use
its PnL/`alert_loss` as a safety measure for such inventory.

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

- Have the release owner review and sign off the production canary evidence.
- If foreign/manual orders must be protected during the first live session,
  repeat the canary with a deliberately identified non-maker order under the
  venue's emergency-cancel procedure.
- Keep the venue/network-originated disconnect scenario as a follow-up if the
  venue can provide a safe server-side fault trigger.

For a future paper run, capture stdout/stderr without a signal-sensitive pipe
so that the long-run's own `lifecycle: stopped` line is retained.

The gate is unlocked, but every live session remains subject to the supervised
canary procedure, emergency cancel readiness, and immediate re-lock on any
strategy, venue API, credential, or risk-control change.
