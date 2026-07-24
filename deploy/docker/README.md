# Stage 2 A/B orchestrator — docker-compose

Containerized equivalent of `deploy/systemd/standx-maker-stage2-ab.service`.
**Read [`docs/19-maker-stage2-live-ab-runbook.md`](../../docs/19-maker-stage2-live-ab-runbook.md) first.**
This service trades live and must not start until the canary evidence is
recorded and the exact authorization text is in the release record.

## What runs where

- **This compose file**: only the A/B orchestrator (`run_maker_stage2_ab.sh`),
  its observed wrapper, manifest tooling, and the deadman-alert install.
- **Not here**: OpenObserve. Keep running
  [`deploy/openobserve/compose.yaml`](../openobserve/compose.yaml) separately.
  `network_mode: host` lets this container reach it on `127.0.0.1:5080`.

## Prerequisites

1. A **clean checkout at the release commit** (the image build fails if the
   committed strategy source — `crates/*`, `Cargo.*`, `examples/maker.toml` —
   is dirty, because the run manifest would reject every arm).
2. **Env-only auth** — this container never reads `credentials.enc`. Set
   `STANDX_JWT` and `STANDX_PRIVATE_KEY` directly in
   `/etc/standx/maker-stage2-ab.env` (step 3 below); the entrypoint fails
   closed (exit 64) if either is missing. Live trading requires a private key
   for order signing, so both are mandatory, not just the JWT.
3. A root-owned `0600` `/etc/standx/maker-stage2-ab.env` — copy
   [`deploy/systemd/maker-stage2-ab.env.example`](../systemd/maker-stage2-ab.env.example),
   fill secrets (including `STANDX_JWT` / `STANDX_PRIVATE_KEY`) and the three
   `STANDX_BASELINE_*` metadata values. The `/opt/standx` paths it targets are
   correct as-is, but **override the two lock paths** for docker (the
   example's `/run/lock/...` defaults are for the systemd deployment, which
   shares them with the host):
   ```
   STANDX_MAKER_LOCK_PATH=/opt/standx/var/lock/standx-maker-live.lock
   STANDX_STAGE2_AB_LOCK_PATH=/opt/standx/var/lock/standx-maker-stage2-ab.lock
   ```
   Only keep the `/run/lock/...` defaults (and re-add the `/run/lock` bind
   mount removed from `docker-compose.yml`) if a host-run
   `standx-maker.service` / `standx-maker-stage2-ab.service` runs at the same
   time as this container and must be mutually exclusive with it. See "Do not"
   below.
4. The frozen `examples/maker-stage2-xag-{baseline,candidate}.toml` (shipped in
   the image).

Point compose at the log dir with a sibling `.env` (or export the var):

```
STANDX_LOG_DIR_HOST=/opt/standx/var/standx
```

## Run

```bash
cd deploy/docker

# 1. Build + config/preflight only, no live orders (maps to STANDX_STAGE2_VALIDATE_ONLY):
docker compose --profile ab run --rm \
  -e STANDX_STAGE2_VALIDATE_ONLY=1 stage2-ab

# 2. Start the guarded two-hour A/B (only after canary evidence is accepted):
docker compose --profile ab up -d --build

# 3. Follow:
docker compose --profile ab logs -f

# 4. Stop with clean arm cleanup (SIGTERM -> orchestrator -> live arm cancel-all):
docker compose --profile ab stop        # honors the 120s grace period
```

The `ab` profile means a bare `docker compose up` starts nothing — live trading
only launches when you pass `--profile ab` explicitly.

## systemd → compose mapping

| systemd unit | compose | notes |
|---|---|---|
| `ExecStart=run_maker_stage2_ab.sh` | entrypoint `exec`s it | unchanged orchestrator |
| `ExecStartPre=openobserve_alerts.py` | entrypoint, fail-closed | deadman alert installed before any order |
| `Restart=on-failure` + `RestartPreventExitStatus=75` | `restart: "no"` | **behavior change — see below** |
| `Conflicts=standx-maker.service` | not enforced by default | see "Changed" below — container uses container-local locks unless you opt in to sharing `/run/lock` |
| `KillSignal=SIGTERM` / `TimeoutStopSec=90` | `stop_signal` + `stop_grace_period: 120s` | plus a new orchestrator SIGTERM trap |
| `EnvironmentFile=/etc/standx/maker-stage2-ab.env` | `env_file` | same file |
| `OnFailure=standx-notify@…` | `STANDX_SUPERVISOR_WEBHOOK` critical post | orchestrator already webhooks on critical stop |
| `~/.local/share/standx/credentials.enc` (file auth) | `STANDX_JWT` + `STANDX_PRIVATE_KEY` in `env_file` (env-only auth) | **behavior change** — this container never mounts or reads `credentials.enc`; the systemd deployment is unaffected |

## Preserved vs. changed safety semantics

**Preserved**
- Exit 75 critical stop, no automatic flatten, arm invalidation, manifest +
  venue empty-order/empty-position gates between arms — all in the unchanged
  orchestrator.
- Deadman alert installed before the first live order (fail-closed entrypoint).
- Clean shutdown: a **new SIGTERM/SIGINT trap** in `run_maker_stage2_ab.sh`
  forwards the signal to the active arm so `docker stop` triggers the normal
  freeze/cancel-all instead of orphaning a live maker. Without this, docker's
  PID-1 signal would have killed the orchestrator and left live orders.

**Changed — operator must accept**
- **No auto-restart** (`restart: "no"`). systemd restarted up to 3×/300s on
  transient failure but never on exit 75. Docker restart policies cannot exclude
  an exit code, so auto-restart would relaunch after a critical stop that may
  have left an open position — unacceptable. Every stop here (critical or
  transient) requires a human to clear per the runbook emergency procedure.
- **Host networking + root in container** for telemetry/venue parity. This
  is dedicated-host infra, matching the current host-process deployment; it is
  not isolated the way a bridged, unprivileged container would be.
- **Locks are container-local by default, not shared with `/run/lock`.**
  Bind-mounting a system directory like the host's `/run/lock` hit
  environment-specific failures in practice (permission/SELinux denials on
  some hosts) for no benefit when nothing else on the host is running a live
  maker. `STANDX_MAKER_LOCK_PATH` / `STANDX_STAGE2_AB_LOCK_PATH` should point
  under `/opt/standx/var/lock` (see Prerequisites). **This means the
  container no longer guarantees mutual exclusion with a host-run
  `standx-maker.service`.** If you do run one alongside this container, bind
  mount a shared host lock directory into both and point both deployments'
  lock env vars at it.

## Do not

- Do not pass `--controlled-disconnect-after` through this path — it forces a
  fail-safe shutdown, which the orchestrator treats as an arm exiting early
  (critical stop). Run that canary drill manually per the runbook.
- Do not run this container and a host `standx-maker.service` at the same
  time unless you've bind-mounted a shared lock directory into both (see
  "Locks are container-local by default" above) — with container-local locks,
  nothing stops the two from trading the same symbol concurrently.

## Troubleshooting

Issues hit (and fixed) during the first real rollout, in the order they
surface:

- **`docker build` fails: `feature \`edition2024\` is required`** — the
  builder image was pinned below Rust 1.85 (where `edition2024` stabilized),
  and `Cargo.lock`'s transitive deps now need it. Fixed by pinning the
  builder to a current `rustN.N-bookworm` tag; if it recurs after a
  `Cargo.lock` update, bump the Dockerfile's `FROM rust:...` further.
- **`openobserve alerts error: ... GET /api/{org}/{stream}/alerts returned
  HTTP 404`** — OpenObserve's alerts API moved to `/api/v2/{org}/alerts`
  (org-scoped, keyed by `alert_id`) in current builds; this is not a
  stream-existence problem (creating the stream first does not help — the
  old endpoint is simply gone). Fixed in `scripts/openobserve_alerts.py`.
  If you pull a much newer OpenObserve image and this recurs, the API may
  have moved again — check its own `/api-doc/openapi.json` for the current
  alerts paths rather than guessing.
- **`failed to open live lock /run/lock/standx-maker-stage2-ab.lock`** — the
  container bind-mounted the host's `/run/lock`, which failed to open on some
  hosts (permission/SELinux denial, host-specific). Fixed by moving to
  container-local locks under `/opt/standx/var/lock` (see "Locks are
  container-local by default" above); make sure
  `/etc/standx/maker-stage2-ab.env` sets `STANDX_MAKER_LOCK_PATH` /
  `STANDX_STAGE2_AB_LOCK_PATH` there, not under `/run/lock`.
- **Stale image after a code/script change** — `docker compose run`/`up`
  reuse an existing local image tag and do **not** rebuild automatically. If
  a fix doesn't seem to take effect, rebuild explicitly:
  `docker compose --profile ab build --no-cache` (or add `--build` to the
  `run`/`up` command).
