#!/usr/bin/env bash
# Container entrypoint for the Stage 2 A/B orchestrator.
#
# Mirrors the systemd unit's start sequence:
#   ExecStartPre=openobserve_alerts.py   -> install the external deadman alert
#   ExecStart=run_maker_stage2_ab.sh     -> the guarded A/B orchestrator
# Both preconditions fail closed: if the credential mount or the deadman alert
# is not in place, the container refuses to start rather than trading blind.
set -euo pipefail

root="${STANDX_INSTALL_ROOT:-/opt/standx}"

# Validate-only is a pure offline config/preflight check (symbol, config
# byte-equality, hashes) — no credentials, no OpenObserve, no live orders. Skip
# the live-only preconditions and hand straight to the orchestrator, which
# validates and exits 0.
if [[ "${STANDX_STAGE2_VALIDATE_ONLY:-0}" == "1" ]]; then
  exec "$root/scripts/run_maker_stage2_ab.sh"
fi

# Env-only auth: this deployment authenticates strictly from the environment,
# never a credentials.enc file (the file mount was removed). Both vars are
# required — live trading signs orders, so STANDX_PRIVATE_KEY is not optional
# here (the maker refuses live with an empty private key). Fail closed so a
# missing/partial credential surfaces here rather than mid-run.
if [[ -z "${STANDX_JWT:-}" || -z "${STANDX_PRIVATE_KEY:-}" ]]; then
  printf 'entrypoint: env-only auth: set STANDX_JWT and STANDX_PRIVATE_KEY (see env_file)\n' >&2
  exit 64
fi

# Install the OpenObserve deadman alert before any live order. This is the
# runbook-required "no telemetry for N minutes" backstop; a failure here is a
# gate failure, so do not fall through to trading.
if [[ "${STANDX_STAGE2_SKIP_ALERT_INSTALL:-0}" != "1" ]]; then
  printf 'entrypoint: installing OpenObserve deadman alert\n' >&2
  python3 "$root/scripts/openobserve_alerts.py"
else
  printf 'entrypoint: WARNING deadman alert install skipped by STANDX_STAGE2_SKIP_ALERT_INSTALL=1\n' >&2
fi

# exec so the orchestrator becomes the signal target; its SIGTERM trap forwards
# to the active live arm for normal freeze/cancel-all cleanup on `docker stop`.
exec "$root/scripts/run_maker_stage2_ab.sh"
