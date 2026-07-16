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
2. Live credentials on the host via `standx auth login`
   (`~/.local/share/standx/credentials.enc`), mounted read-only.
3. A root-owned `0600` `/etc/standx/maker-stage2-ab.env` — copy
   [`deploy/systemd/maker-stage2-ab.env.example`](../systemd/maker-stage2-ab.env.example),
   fill secrets and the three `STANDX_BASELINE_*` metadata values. The in-container
   paths it already targets (`/opt/standx`, `/run/lock/...`) are correct as-is.
4. The frozen `examples/maker-stage2-xag-{baseline,candidate}.toml` (shipped in
   the image).

Point compose at the host credential file and log dir with a sibling `.env`
(or export the vars):

```
STANDX_CREDENTIALS_FILE=/home/youruser/.local/share/standx/credentials.enc
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
| `Conflicts=standx-maker.service` | shared `/run/lock` bind mount | flock keeps host maker + container mutually exclusive |
| `KillSignal=SIGTERM` / `TimeoutStopSec=90` | `stop_signal` + `stop_grace_period: 120s` | plus a new orchestrator SIGTERM trap |
| `EnvironmentFile=/etc/standx/maker-stage2-ab.env` | `env_file` | same file |
| `OnFailure=standx-notify@…` | `STANDX_SUPERVISOR_WEBHOOK` critical post | orchestrator already webhooks on critical stop |

## Preserved vs. changed safety semantics

**Preserved**
- Exit 75 critical stop, no automatic flatten, arm invalidation, manifest +
  venue empty-order/empty-position gates between arms — all in the unchanged
  orchestrator.
- Host-wide single-live-maker guarantee, via the shared `/run/lock` mount and
  the existing `flock` guard/maker locks.
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
- **Host networking + root in container** for lock/telemetry/venue parity. This
  is dedicated-host infra, matching the current host-process deployment; it is
  not isolated the way a bridged, unprivileged container would be.

## Do not

- Do not pass `--controlled-disconnect-after` through this path — it forces a
  fail-safe shutdown, which the orchestrator treats as an arm exiting early
  (critical stop). Run that canary drill manually per the runbook.
- Do not run this container and the host `standx-maker.service` at the same
  time; the shared lock will reject the second, but do not rely on it — stop
  one first.
