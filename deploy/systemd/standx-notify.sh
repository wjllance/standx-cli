#!/usr/bin/env bash
# OnFailure notifier for StandX supervision units.
#
# Argument $1 is the failed unit name (passed as %i from the
# `OnFailure=standx-notify@%n.service` hook). Always logs to stderr (the
# journal); additionally POSTs to STANDX_SUPERVISOR_WEBHOOK when it is set and
# curl is available. Best-effort: a failed webhook never changes the outcome.
set -uo pipefail

unit="${1:-unknown.unit}"
host="$(hostname 2>/dev/null || echo unknown-host)"
now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
msg="StandX supervision alert: unit ${unit} entered a failure state on ${host} at ${now}. Inspect with: journalctl -u ${unit} -n 100 --no-pager"

printf '%s\n' "$msg" >&2

if [[ -n "${STANDX_SUPERVISOR_WEBHOOK:-}" ]] && command -v curl >/dev/null 2>&1; then
  escaped="${msg//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  curl -fsS -m 5 -X POST \
    -H 'Content-Type: application/json' \
    --data "{\"text\":\"$escaped\"}" \
    "$STANDX_SUPERVISOR_WEBHOOK" >/dev/null 2>&1 ||
    printf 'notify webhook post failed (message logged above)\n' >&2
fi
