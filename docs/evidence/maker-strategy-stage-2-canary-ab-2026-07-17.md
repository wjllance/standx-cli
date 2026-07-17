# Maker Stage 2 v0 canary and live A/B record — 2026-07-17 (docker)

## Authorization

Recorded verbatim in the release record, provided by the release owner on
2026-07-17 (Asia/Shanghai), before any live action in this session:

> 授权执行 XAG-USD size=0.01 max_position=0.2 的阶段2 canary 与2小时A/B

Scope: XAG-USD only, `size=0.01`, one level, `max_position=0.2`, canary plus
the guarded two-hour baseline/candidate time-slice A/B. This does not
authorize another symbol, larger exposure, active inventory exit, or automatic
flatten. Execution environment: docker (`deploy/docker/docker-compose.yml`,
profile `ab`), per the release owner's instruction "在docker中执行".

## Prior attempt and cleanup (must precede any retry)

- Container `standx-maker-stage2-ab` ran the A/B orchestrator from
  2026-07-16T16:48:40Z to 2026-07-16T19:19:07Z (image
  `standx-stage2-ab:latest`, git `a4080be46ce63bd56c9c6090816392131589374d`).
- Baseline arm `stage2-baseline-20260716T164841Z-3df955e967fa` ran 9016s
  (3242 cycle summaries, 5 passive fills) and ended with ledger position
  `-0.03`. The orchestrator waited the full 1800s flat grace, did not flatten
  (by design), declared the arm **invalid** ("position remained nonzero beyond
  flat grace"), sent the critical webhook, and exited 75.
- Manifest: `/opt/standx/var/standx/stage2-baseline-20260716T164841Z-3df955e967fa.manifest.json`
  (status `invalid`; strategy_source_clean=true; symbol metadata complete).
  Per the roadmap, this arm's data is void for A/B comparison.
- Manual residual-position disposal per the runbook emergency procedure was
  completed by the named operator. Independent re-check on
  2026-07-17T01:10Z (read-only venue queries inside the image, host
  credentials mounted read-only): `account orders --symbol XAG-USD` → `[]`,
  `account positions --symbol XAG-USD` → `[]`. Venue flat, no residual maker
  orders. Retry gate cleared.

## Frozen artifacts

- git SHA: `a4080be46ce63bd56c9c6090816392131589374d` (HEAD, strategy source
  clean per the Dockerfile build gate).
- Baseline config SHA-256:
  `3df955e967fa97c92557b545c6eae52b5ff27dc5fd323d5e813eb89aaa04d146`
  (`examples/maker-stage2-xag-baseline.toml`, re-verified 2026-07-17).
- Candidate config SHA-256:
  `30fdd415efcc2b57f7a246f4344e9929790b95d5fd9a7a7c49f7e21b7bef891d`
  (`examples/maker-stage2-xag-candidate.toml`, re-verified 2026-07-17).
- Environment: `/etc/standx/maker-stage2-ab.env` (root 0600) with docker
  container-local locks (`/opt/standx/var/lock/…`), baseline symbol metadata
  (`price_tick_decimals=2`, `qty_tick_decimals=3`, `min_order_qty=0.001`),
  OpenObserve + supervisor webhook (feishu) present.
- Mutual exclusion: host `standx-maker.service`,
  `standx-maker-stage2-ab.service`, `standx-openobserve-catchup.service` all
  inactive; OpenObserve itself runs as a separate container (up 6 days).

## Webhook probes

- Image `standx-stage2-ab:latest` id `02c2775d05b9`; bundled binary SHA-256
  `2333f6ad176d168682956b7a05fdf62ed74566bcb5a93d5821768d6659b21cd6` (matches
  the prior attempt's manifest program hash — same commit, reproducible build).
- Docker validate-only preflight passed 2026-07-17T01:1xZ:
  `stage2 A/B validation ok: symbol=XAG-USD baseline=3df955e9… candidate=30fdd415…`.
- Four probes sent 2026-07-17T01:1xZ with
  `test_id=stage2-webhook-ccf00583cabf` (stop_loss, position_risk, equity,
  margin; all HTTP 2xx). Receiver confirmation: **confirmed** by the release
  owner — all four messages received in the feishu receiver.
- Named operator wujunlin confirmed ready with authenticated venue access for
  the first 30 minutes of the canary window.

## Canary evidence

Both canaries executed in docker (`docker compose --profile ab run --rm`,
image `02c2775d05b9`, env from `/etc/standx/maker-stage2-ab.env`, credentials
mounted read-only) on 2026-07-17T01:23Z:

1. **ws-command-canary** — full create/cancel correlation chain retained:
   `client_order_id=sxmk-canary-abe24600ea55`, `order_id=11630688166`,
   create `request_id=15d64566-8806-4886-8313-db04becd0d9d` (accepted, code 0),
   cancel `request_id=7459ccf2-36b8-4621-937d-59b8368cafc1` (accepted, code 0);
   `order_visible` → `absence_verified` → `position_verified=0.0`.
   Venue-minimum post-only 0.001 XAG @ 55.09.
2. **Controlled-disconnect canary** — run_id `stage2-canary-20260717T012339Z`,
   candidate config (`adaptive_spread_enabled=true`). Observed sequence exactly
   as required: order-response fault injected at 15s → `disconnected_frozen`
   (warning) → `maker_cleanup complete remaining_maker_orders=0` →
   `reconnect_unavailable` (critical, refusing further live orders) → final
   `maker_cleanup remaining_maker_orders=0` → critical `fail_safe` stop →
   `lifecycle stopped` (6 cycles, 0 fills) → **exit 75** (expected drill
   outcome). Both place requests closed as `effective` (ack p50 217ms,
   effective p50 153ms); no unexplained pending requests. All 34 NDJSON events
   uploaded to OpenObserve by the wrapper.
3. **Manifest validation** —
   `/opt/standx/var/standx/stage2-canary-20260717T012339Z.manifest.json`:
   all 14 required checks true (`cycle_sequence_complete`,
   `lifecycle_started/stopped`, `strategy_source_clean`,
   `symbol_metadata_complete`, …); status `finished`; `final_position=0.0`;
   log SHA-256 `a503b6ce…` matches. The sole validator complaint is
   "manifest is not baseline eligible", which is by design for a fail-safe
   drill (short window, exit 75) — it is gate evidence, not comparison data.
4. **Independent post-check** 2026-07-17T01:2xZ (read-only queries):
   `account orders --symbol XAG-USD` → `[]`,
   `account positions --symbol XAG-USD` → `[]`.

Note (non-blocking): in-container `git status` shows untracked runtime paths
(credentials mount, var/lock, `.mimocode/.cron-lock`, this evidence file baked
into the image). Only strategy paths are gated; `strategy_source_clean=true`.
Consider adding those paths to `.dockerignore` after the A/B window.

## Two-hour A/B

- Started 2026-07-17T01:27Z via `docker compose --profile ab up -d --build`
  (container `standx-maker-stage2-ab`, image `02c2775d05b9`).
- Startup sequence verified: deadman alert installed before any order →
  preflight `orders=[] positions=[]` → baseline arm start.
- Baseline arm: `stage2-baseline-20260717T012722Z-3df955e967fa`
  (config hash `3df955e9…`, `adaptive_spread_enabled=false`), first cycles
  healthy: two-sided quotes, WS market feed, OpenObserve live upload.
- Candidate arm: **never started** (see incident below).
- Result: **baseline arm invalid, A/B stopped (exit 75) at 2026-07-17T03:58Z.**

## Incident 2026-07-17: second non-flat arm + SIGTERM cleanup gap

- Baseline arm ran 9015s (3385 cycles, 17 fills, range 169.6bps), ended with
  ledger position `-0.03`. The 1800s flat grace expired without natural
  return to zero → orchestrator invalidated the arm
  ("position remained nonzero beyond flat grace"), sent the critical webhook,
  exited 75 without flattening (by design). Manifest
  `/opt/standx/var/standx/stage2-baseline-20260717T012722Z-3df955e967fa.manifest.json`.
- **SIGTERM cleanup gap (fail-closed deviation)**: at grace expiry the
  orchestrator SIGTERMs the arm; the wrapper forwards SIGTERM to the maker,
  but the maker only handles SIGINT (`tokio::signal::ctrl_c()`,
  `runtime/state.rs:154`). SIGTERM killed it with the default disposition
  (exit 143) — no `maker_cleanup`, no terminal lifecycle event. The NDJSON
  ends mid-quoting (last events: cancel+place pair at 03:57:08Z). Two
  current-run orders survived on the venue; the buy leg filled at 04:04:40Z
  (+0.01), leaving a resting sell `11633410129` (0.01 @ 55.52) and a residual
  short `-0.02`. The same exit=143 pattern occurred on 2026-07-16, so both
  arm boundaries left residue that was only removed by manual intervention.
- Emergency procedure executed 2026-07-17T04:1xZ with the release owner's
  explicit instruction: `order cancel-all XAG-USD` → orders `[]`; venue
  position re-queried (exact `-0.02`, unchanged since 04:04:40Z); one
  reviewed reduce-only market buy 0.02 (order id
  `40268377-b67a-4914-8e26-79cbe8ab1511`, executed by the agent under
  one-time authorization — a deviation from the runbook's operator-only
  default, recorded here); final state `orders=[] positions=[]`.
- Runbook doc bug found: the emergency commands say `standx order new …` but
  the CLI subcommand is `standx order create …`.
- Structural observation for the retry decision: with ~15–17 fills per 2h
  arm and two-sided quoting throughout the grace window, ending exactly flat
  is a low-probability outcome (~fill-parity), so most arms would fail a
  hard-clock flat gate. **Resolved in parallel**: commit `fa00c29` (BossX,
  2026-07-17T01:57Z) amended the orchestrator protocol — `arm_seconds` (2h)
  is now a minimum; a non-flat arm keeps quoting and switches at the next
  natural flat (warning webhook every 1800s), with a 6h hard cap
  (`STANDX_STAGE2_ARM_MAX_SECONDS`) that still invalidates and exit-75s
  without any auto-flatten. No strategy change. docs/19 was rewritten to
  match. The release owner additionally chose: **fix the SIGTERM cleanup gap
  first, then re-authorize a retry.**

## SIGTERM cleanup-gap fix (2026-07-17, commit `c9306ce`)

- Root cause: the maker's shutdown watcher only awaited
  `tokio::signal::ctrl_c()` (SIGINT). Every supervisor in the deployment
  stack (systemd, `docker stop`, the A/B orchestrator via
  `run_maker_observed.sh`) stops the maker with **SIGTERM**, which took the
  default disposition — instant death (exit 143), no `maker_cleanup`,
  resting orders left on the venue. The `deploy/README.md` claim "SIGTERM →
  fail-safe cleanup → exit 0" never held.
- Fix (`crates/standx-cli/src/commands/maker/runtime/state.rs`): the
  shutdown watcher task now registers explicit unix signal streams for
  **both** SIGINT and SIGTERM (`tokio::signal::unix::signal`) and feeds the
  same latched watch channel, so SIGTERM drives the identical graceful
  `StopRequested → cleanup → exit 0` path as Ctrl+C. Non-unix keeps the
  original `ctrl_c()` loop. No strategy, accounting, or output-contract
  change.
- Debugging note: an earlier revision raced `tokio::signal::ctrl_c()`
  against `sigterm.recv()` in one `select!`; in the real binary that
  formulation never registered handlers (verified via `/proc/<pid>/status`
  SigCgt) while a minimal reproduction did. The explicit two-`Signal`
  formulation registers both handlers deterministically.
- Verification (paper mode, XAG-USD, host binary, no live orders):
  - SIGTERM drill: handler visible in SigCgt; process exits **0** in ~1s
    with `lifecycle stopped` (previously: exit 143, no stop event).
  - SIGINT drill: same graceful path (regression check).
- Offline verification: workspace tests 181 cli + 154 maker + 75 sdk + 31
  unit + 13 integration + 2 main + e2e/doc (2 credential-dependent e2e
  ignored as before) all pass; clippy `-D warnings` clean; `cargo fmt
  --check` clean; `py_compile openobserve_dashboard.py` ok.
- Consequence for the retry: the binary changes, so the frozen program hash
  changes. The retry needs a rebuilt image, a fresh canary pass with the
  new binary, and a **new exact authorization** before the A/B restarts.

## Retry after fixes (2026-07-17T05:2xZ)

- New exact authorization recorded (same scope as the original, the first
  one having been consumed by the failed run):
  > 授权执行 XAG-USD size=0.01 max_position=0.2 的阶段2 canary 与2小时A/B
- Retry artifact: image `standx-stage2-ab:latest` id `2dd03d005184`, git
  `c9306cec98ea7c42526eeaf07307e0df045a617f` (SIGTERM fix `c9306ce` on top of
  orchestrator extend-to-flat `fa00c29`), binary SHA-256
  `1bbb707b97348f7ce8099f889cda3d325b27c9c8f0d7e6263d9f55479d186e9a`;
  Dockerfile clean-source gate passed at build. Config hashes unchanged
  (baseline `3df955e9…`, candidate `30fdd415…`); validate-only preflight
  passed 2026-07-17T05:2xZ.
- Operator wujunlin re-confirmed ready for the canary window.

### Retry canary (new binary) — passed 2026-07-17T05:35Z

- Webhook probes re-sent and receiver-confirmed:
  `test_id=stage2-webhook-193034e4bd69` (all four kinds).
- ws-command-canary: `client_order_id=sxmk-canary-2e027fd9713c`,
  `order_id=11634874724`, create/cancel accepted (request ids
  `006fdc91-…` / `36889edf-…`), absence + flat position verified.
- Controlled-disconnect canary `stage2-canary-20260717T053502Z`: expected
  fail-safe sequence (frozen → cleanup remaining=0 → critical fail_safe →
  exit 75); manifest all 14 checks true, `final_position=0.0`,
  `git_sha=c9306ce`; sole validator note "not baseline eligible" (drill by
  design). Independent post-check: `orders=[] positions=[]`.

### Retry A/B — started 2026-07-17T05:36Z

- `docker compose --profile ab up -d`; deadman alert installed, preflight
  `orders=[] positions=[]`, baseline arm
  `stage2-baseline-20260717T053601Z-3df955e967fa` started.
- Behavior change vs the failed attempts (orchestrator `fa00c29`): the 2h
  mark is now a minimum; a non-flat arm extends to the next natural flat
  (warning webhook every 1800s), 6h hard cap `STANDX_STAGE2_ARM_MAX_SECONDS`
  still invalidates without flattening.
- Result: pending

### Offline arm-switch harness — passed 2026-07-17T05:55Z

User asked for fast confirmation that arm switching works before the first
real 2h switch (~07:36Z). A fully offline harness exercised the **real**
orchestrator logic (`run_maker_stage2_ab.sh` copied to
`/tmp/stage2-harness/` with only `arm_seconds 7200→8` and
`flat_grace_seconds 1800→4` via sed), the **real** `run_maker_observed.sh`
wrapper, and the **real** `maker_run_manifest.py` tooling
(`STANDX_INSTALL_ROOT=<repo>`, frozen configs byte-compare intact), against
a fake `standx` bash binary whose venue position is driven by a state file.
`OPENOBSERVE_AUTO_UPLOAD`/`STANDX_SUPERVISOR_WEBHOOK` explicitly unset; no
venue or network contact; live A/B container untouched (verified `Up`
throughout).

- **S1 — flat at deadline**: baseline arm completed at the window end,
  candidate started, then baseline again (alternation continues); all 3
  manifests `status=finished`, `baseline_eligible=true`,
  `final_position=0.0`; SIGTERM to the orchestrator mid-arm forwarded to
  the arm, graceful exit 0; no CRITICAL.
- **S2 — non-flat at deadline**: extension warning fired
  ("past its 8s window but not flat; extending and will switch at the next
  natural flat"), then after the state flipped flat:
  "reached natural flat after extension; proceeding to switch", arm
  complete with `orders=[] positions=[]`; manifest finished+eligible+flat.
- **S3 — non-flat past hard cap** (`ARM_MAX=20`): repeated extension
  warnings, then `CRITICAL stage2 A/B stopped: arm=baseline stayed
  non-flat past 20s hard cap … no automatic flatten was attempted`,
  orchestrator exit 75, arm manifest invalidated (`status=invalid`).
- No stray harness processes after any scenario. Artifacts (orchestrator
  logs + manifests per scenario): `/tmp/stage2-harness/artifacts-s{1,2,3}/`.

### Retry A/B — terminated by credential expiry 2026-07-17T09:04Z (exit 75)

- Baseline arm `stage2-baseline-20260717T053601Z-3df955e967fa` ran 05:36Z →
  09:04Z (4606 cycles, 11 fills, uptime ~90%, session PnL ≈ -0.01); crossed
  the 2h minimum at 07:36Z non-flat and extended per the extend-to-flat
  design (extension warnings at ~07:36/08:06/08:36Z, position decaying
  -0.02 → -0.01). Candidate arm never started.
- **Root cause: venue credential token expired mid-arm.** First symptom
  ~09:04Z: orchestrator's `account positions` poll began failing
  ("Authentication required", exit/JSON invalid) → extension loop's
  fail-closed path SIGTERMed the arm. The arm's own cleanup (cancel-all)
  then failed 3/3 attempts with the same "Authentication required" →
  `risk_notification critical/residual_orders`, maker lifecycle stopped
  with "cleanup failed" and exit 75; orchestrator `CRITICAL … could not
  prove venue position state … no automatic flatten was attempted`,
  exit 75. Container `Exited (75)`, NOT restarted (restart: "no" held).
- Post-incident read-only venue checks (09:08–09:10Z, docker run
  credentials.enc:ro): `account orders` and `account positions` BOTH fail
  with "Authentication required" — account-state API visibility is down
  until re-login. Market data (public) unaffected.
- Last known state: expected_position=-0.01 XAG (≈$0.55; maker ledger,
  fills_total=11 unchanged since 08:38Z). Residual orders UNKNOWN — the
  final quote pair could not be cancelled (auth), so one bid+ask pair of
  0.01 XAG each may still be resting on the book.
- Handoff to operator (wujunlin): 1) `standx auth login` on the host to
  refresh credentials.enc; 2) verify `account orders`/`account positions`;
  3) cancel any residual sxmk- orders and flatten the -0.01 if still
  present; 4) only then consider a fresh A/B launch (preflight requires
  orders=[] positions=[]). NO automatic flatten or restart was attempted
  by tooling, per runbook.
- **Ops gap surfaced**: token lifetime (~3.5h from 05:36Z auth) is shorter
  than a 2h+extension arm can be. Before the next run: refresh credentials
  immediately before launch and/or schedule a mid-run re-login; consider
  documenting expected token TTL in the runbook. Monitoring cron
  5d7f4274 deleted at incident detection.

### Wind-down arm switching — implemented and offline-verified 2026-07-17T10:2xZ

Motivated by the 09:04Z incident (baseline arm extended 1.5h without
converging) and operator request for deterministic switch-time position
closure. Replaces extend-to-flat quoting with a latched **wind-down**:

- `standx-maker`: `CycleInput.wind_down`/`qty_tolerance`; `plan_cycle` in
  wind-down never desires new quotes (even once flat — no re-accumulation)
  and plans a full reduce-only exit for any residual above tolerance,
  ignoring configured exit thresholds (frozen configs have them disabled).
  Vol-halt still suppresses the taker exit. New unit tests 35-37; maker
  suite 157 passed.
- `standx-cli`: SIGUSR1 handler registered alongside SIGINT/SIGTERM
  (latched watch channel); each cycle latches `wind_down`, emits a
  `lifecycle`/`wind_down` JSON line + webhook once, and threads the flag
  into the planner. Existing reduce-only market-exit path
  (`cycle.rs:762+`) executes the flatten.
- `run_maker_observed.sh` forwards USR1; `run_maker_stage2_ab.sh` sends
  USR1 at the 2h mark and re-sends with each 30-min warning. Flat poll,
  6h hard cap (now "stalled wind-down"), manifest validation, postcheck,
  and no-auto-flatten semantics unchanged.
- No new config keys; frozen configs and byte-compare untouched.
- Offline gates: full workspace tests green (10 suites), clippy
  `-D warnings`, fmt, py_compile, bash -n all clean.
- Harness (fake standx + shrunk-timer orchestrator, scratch repo copy so
  the manifest `strategy_source_clean` gate passes with the real tree
  dirty): **s4 wind-down happy path PASS** (USR1 → flatten → arm complete
  → candidate starts), s1 immediate-switch PASS, s2 stalled-wind-down →
  flat → switch PASS, s3 stalled wind-down past hard cap → CRITICAL
  exit 75 PASS. Artifacts `/tmp/stage2-harness/artifacts-s{1..4}/`.
- Deployment constraint (also in docs/19): orchestrator, wrapper, and
  maker binary must ship in the same image — SIGUSR1 to a pre-wind-down
  binary kills it.
- Status: implemented and verified offline; NOT yet committed (awaiting
  operator approval), not yet deployed.

### Wind-down deployed; HYPE A/B #1 incident and cycle_summary fix — 2026-07-17T10:4x–15:1xZ

Committed as ba60b6a + 08c08ed (PR #316, exp -> main). Docker image rebuilt
from 08c08ed (build-time strategy_source_clean gate passed).

- 10:44Z XAG A/B restarted (pre-checks: venue flat, token TTL 166h — the
  09:04Z failure was server-side session invalidation, not JWT expiry).
  Arm 1 healthy; operator then redirected to HYPE; XAG container stopped
  cleanly 10:4xZ (Exited 0, venue flat).
- HYPE bring-up: `/etc/standx/maker-stage2-hype-ab.env` installed (venue
  metadata re-verified live: price_tick=3, qty_tick=2, min_order=0.1).
  ws-command-canary PASS (full create/cancel chain, order 11639733982);
  controlled-disconnect drill PASS (frozen -> cleanup empty -> fail-safe
  exit 75); canary manifest validated (log sha256 1177c1f3... matches).
- 10:52Z HYPE A/B #1 started. Baseline arm ran the full 2h (34+ fills,
  PnL -0.25). 12:52Z SIGUSR1 wind-down: quotes pulled, residual +0.2
  flattened via reduce-only market exit @59.735, position -> 0.0, maker
  exit 0. **Wind-down's first live exercise: flatten worked exactly as
  designed.**
- BUT the orchestrator then critical-stopped (exit 75): the exit order
  spent one cycle awaiting venue confirmation, that cycle aborted via the
  duplicate-exit guard BEFORE emitting cycle_summary, and the manifest
  gate (correctly, fail-closed) failed `cycle_sequence_complete`
  (`missing_cycles=[2663]`, 2668/2669 present, otherwise fully eligible).
  Arm 1 data void; arm 2 never ran. Venue verified flat.
- Root cause: `cycle.rs` treated "exit still awaiting confirmation" as a
  hard cycle error, skipping the summary emission. Fix (2db6106): the
  cycle now completes with zero order work (identical execution
  semantics — no duplicate exit, no quote churn) and emits its summary;
  the historical `inventory_exit/failed` notification is preserved
  byte-identical on the success path. Also afbb695 makes arm_seconds
  env-configurable (`STANDX_STAGE2_ARM_SECONDS`, default 7200 unchanged).
  Offline gates green; validate-only + negative-path checks pass.
- 13:53Z HYPE A/B #2 (30-min arms, operator-directed validation rerun)
  on image afbb695. Arm 1 baseline: wind-down at 14:23Z with residual
  +0.2 — flatten @60.392, the awaiting-confirmation cycle (967) emitted
  its summary, manifest `baseline_eligible=True`, `missing_cycles=[]`
  (949 cycles / 1821s). Arm 2 candidate: wind-down at 14:53Z already
  flat, clean stop, manifest `eligible=True`, `missing_cycles=[]`
  (750 cycles / 1820s). Both switch styles (flatten / already-flat) PASS.
- Operator stopped the run 15:08Z (SIGTERM, Exited 0). Arm 3 (baseline
  #2, 371 cycles, void) had residual +0.3 HYPE long; orders=[] but
  position non-flat. Per operator authorization, flattened via host CLI
  reduce-only market sell 0.3 (order f8be1140-5830-42d1-a29c-4d0c69806a32);
  post-check orders=[] positions=[]. Monitoring cron 23033b6b deleted.
- Net: wind-down arm switching is now proven live on HYPE for both the
  flatten and already-flat switch paths, with both arm manifests fully
  baseline-eligible. Remaining follow-ups: restore
  `STANDX_STAGE2_ARM_SECONDS=7200` for the standard 2h A/B; PR #316
  awaits merge; formal 2h baseline/candidate comparison still to be run.
