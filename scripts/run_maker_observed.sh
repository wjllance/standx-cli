#!/usr/bin/env bash
set -uo pipefail

if [[ $# -lt 1 ]]; then
  printf 'usage: %s <standx maker command...>\n' "$0" >&2
  exit 64
fi

args=("$@")
has_maker=0
has_run=0
has_json=0
config_file=""

for ((i = 0; i < ${#args[@]}; i++)); do
  arg="${args[$i]}"
  [[ "$arg" == "maker" ]] && has_maker=1
  [[ "$arg" == "run" ]] && has_run=1
  [[ "$arg" == "--output=json" ]] && has_json=1
  if [[ "$arg" == "--output" || "$arg" == "-o" ]]; then
    if ((i + 1 < ${#args[@]})) && [[ "${args[$((i + 1))]}" == "json" ]]; then
      has_json=1
    fi
  fi
  if [[ "$arg" == "--maker-config" ]] && ((i + 1 < ${#args[@]})); then
    config_file="${args[$((i + 1))]}"
  elif [[ "$arg" == --maker-config=* ]]; then
    config_file="${arg#--maker-config=}"
  fi
done

if ((has_maker == 0 || has_run == 0 || has_json == 0)); then
  printf 'error: command must be a maker run with --output json\n' >&2
  exit 64
fi

umask 077
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
log_dir="${STANDX_LOG_DIR:-$repo_root/var/standx}"
run_id="${STANDX_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
stdout_log="$log_dir/$run_id.ndjson"
stderr_log="$log_dir/$run_id.stderr.log"
pipe_dir=""
child_pid=""

mkdir -p "$log_dir"
pipe_dir="$(mktemp -d "$log_dir/.pipes.XXXXXX")"
mkfifo "$pipe_dir/stdout" "$pipe_dir/stderr"

cleanup() {
  [[ -n "$pipe_dir" && -d "$pipe_dir" ]] && rm -rf "$pipe_dir"
}

forward_signal() {
  local signal="$1"
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    kill -"$signal" "$child_pid" 2>/dev/null || true
  fi
}

trap cleanup EXIT
trap 'forward_signal INT' INT
trap 'forward_signal TERM' TERM

tee "$stdout_log" <"$pipe_dir/stdout" &
stdout_tee_pid=$!
tee "$stderr_log" <"$pipe_dir/stderr" >&2 &
stderr_tee_pid=$!

"${args[@]}" >"$pipe_dir/stdout" 2>"$pipe_dir/stderr" &
child_pid=$!
wait "$child_pid"
child_status=$?
wait "$stdout_tee_pid" || true
wait "$stderr_tee_pid" || true

git_sha="$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || true)"
config_hash=""
if [[ -n "$config_file" && -f "$config_file" ]]; then
  config_hash="$(shasum -a 256 "$config_file" | awk '{print $1}')"
fi

printf 'maker run_id=%s exit=%s stdout=%s stderr=%s\n' \
  "$run_id" "$child_status" "$stdout_log" "$stderr_log" >&2

if [[ "${OPENOBSERVE_AUTO_UPLOAD:-0}" == "1" ]]; then
  if [[ -z "${OPENOBSERVE_USER:-}" || -z "${OPENOBSERVE_PASSWORD:-}" ]]; then
    printf 'OpenObserve upload skipped: credentials are not exported\n' >&2
  else
    upload_args=("$script_dir/openobserve_ingest.py" "$stdout_log" --run-id "$run_id")
    [[ -n "$git_sha" ]] && upload_args+=(--git-sha "$git_sha")
    [[ -n "$config_hash" ]] && upload_args+=(--config-hash "$config_hash")
    python3 "${upload_args[@]}" || printf 'OpenObserve upload failed; raw logs are intact\n' >&2
  fi
fi

exit "$child_status"
