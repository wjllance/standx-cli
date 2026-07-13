#!/usr/bin/env bash
# Boot/crash catch-up: re-upload any un-ingested tail of local maker logs.
#
# On SIGKILL / power loss the local NDJSON survives but the tail past the last
# checkpoint was never sent, so the dashboard is stale until someone runs a
# manual upload. This script closes that gap: for every `<run_id>.ndjson` in
# the log directory it runs the ingester in incremental (non-follow) mode with
# `--run-id <run_id>`, which reuses the exact same checkpoint key the live
# follow uploader used and therefore resumes without re-sending events.
#
# It is safe to run repeatedly (idempotent) and at boot before the maker
# starts. Requires OPENOBSERVE_* to be exported (or sourced from an env file).
set -uo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
log_dir="${STANDX_LOG_DIR:-$repo_root/var/standx}"

# Optionally source an env file (e.g. deploy/openobserve/.env) so the catch-up
# can run from a boot unit without a pre-populated shell.
env_file="${OPENOBSERVE_ENV_FILE:-}"
if [[ -n "$env_file" && -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$env_file"
  set +a
fi

if [[ -z "${OPENOBSERVE_USER:-}" || -z "${OPENOBSERVE_PASSWORD:-}" ]]; then
  printf 'openobserve catch-up skipped: OPENOBSERVE_USER/OPENOBSERVE_PASSWORD not set\n' >&2
  exit 0
fi

if [[ ! -d "$log_dir" ]]; then
  printf 'openobserve catch-up: no log directory at %s; nothing to do\n' "$log_dir" >&2
  exit 0
fi

shopt -s nullglob
logs=("$log_dir"/*.ndjson)
shopt -u nullglob

if ((${#logs[@]} == 0)); then
  printf 'openobserve catch-up: no *.ndjson logs under %s; nothing to do\n' "$log_dir" >&2
  exit 0
fi

git_sha="$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || true)"
failures=0

for log in "${logs[@]}"; do
  run_id="$(basename "$log" .ndjson)"
  printf 'openobserve catch-up: run_id=%s file=%s\n' "$run_id" "$log" >&2
  catchup_args=(
    "$script_dir/openobserve_ingest.py" "$log"
    --run-id "$run_id"
    --incremental
  )
  [[ -n "$git_sha" ]] && catchup_args+=(--git-sha "$git_sha")
  # An exit of 1 here means "no new lines" (already fully ingested) or a bad
  # file; log it and keep going so one stale file cannot block the rest.
  if ! python3 "${catchup_args[@]}"; then
    printf 'openobserve catch-up: nothing to upload or upload failed for run_id=%s\n' "$run_id" >&2
    failures=$((failures + 1))
  fi
done

if ((failures > 0)); then
  printf 'openobserve catch-up: completed with %s file(s) reporting no-new-data or error\n' "$failures" >&2
fi
exit 0
