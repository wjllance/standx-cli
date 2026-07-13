#!/usr/bin/env bash
# launchd exit-code translator for the StandX maker.
#
# launchd's KeepAlive cannot exclude a single exit code from restart the way
# systemd's RestartPreventExitStatus= can. So we run the observed wrapper and
# translate its exit code to match the intended policy, paired with
# `KeepAlive.SuccessfulExit=false` in the plist (restart only on non-zero):
#
#   exit 75  (intentional fail-safe)  -> notify, then exit 0  => launchd does NOT restart
#   exit !=0 (panic / kill / config)  -> notify, then pass through => launchd DOES restart
#   exit 0   (clean stop)             -> exit 0                 => launchd does NOT restart
#
# Usage (from the plist): standx-maker-supervise.sh <standx-bin> --output json maker run ...
set -uo pipefail

FAIL_SAFE_EXIT_CODE=75
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

# Optional env file (OpenObserve creds, STANDX_SUPERVISOR_WEBHOOK, etc.).
env_file="${STANDX_ENV_FILE:-$repo_root/deploy/openobserve/.env}"
if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$env_file"
  set +a
fi

notify() {
  local message="$1"
  printf '%s\n' "$message" >&2
  if [[ -n "${STANDX_SUPERVISOR_WEBHOOK:-}" ]] && command -v curl >/dev/null 2>&1; then
    local escaped="${message//\\/\\\\}"
    escaped="${escaped//\"/\\\"}"
    curl -fsS -m 5 -X POST \
      -H 'Content-Type: application/json' \
      --data "{\"text\":\"$escaped\"}" \
      "$STANDX_SUPERVISOR_WEBHOOK" >/dev/null 2>&1 ||
      printf 'supervisor webhook post failed (message logged above)\n' >&2
  fi
}

host="$(hostname 2>/dev/null || echo unknown-host)"
"$repo_root/scripts/run_maker_observed.sh" "$@"
status=$?

if ((status == FAIL_SAFE_EXIT_CODE)); then
  notify "StandX maker fail-safe shutdown (exit ${FAIL_SAFE_EXIT_CODE}) on ${host}; NOT restarting (intentional). Check the maker logs before relaunching."
  exit 0
elif ((status != 0)); then
  notify "StandX maker unexpected exit (status ${status}) on ${host}; launchd will restart it."
  exit "$status"
fi

exit 0
