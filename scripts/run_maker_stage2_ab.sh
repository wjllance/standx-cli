#!/usr/bin/env bash
set -uo pipefail

root="${STANDX_INSTALL_ROOT:-/opt/standx}"
standx_bin="${STANDX_BIN:-$root/bin/standx}"
baseline_config="${STANDX_STAGE2_BASELINE_CONFIG:-$root/examples/maker-stage2-xag-baseline.toml}"
candidate_config="${STANDX_STAGE2_CANDIDATE_CONFIG:-$root/examples/maker-stage2-xag-candidate.toml}"
symbol="${STANDX_SYMBOL:-XAG-USD}"
arm_seconds=7200
flat_grace_seconds=1800
poll_seconds="${STANDX_STAGE2_POLL_SECONDS:-15}"
log_dir="${STANDX_LOG_DIR:-$root/var/standx}"
ab_lock="${STANDX_STAGE2_AB_LOCK_PATH:-/run/lock/standx-maker-stage2-ab.lock}"

notify() {
  local message="$1"
  local severity="${2:-info}"
  printf '%s\n' "$message" >&2
  if [[ -n "${STANDX_SUPERVISOR_WEBHOOK:-}" ]]; then
    STANDX_STAGE2_NOTIFICATION="$message" \
      STANDX_STAGE2_NOTIFICATION_SEVERITY="$severity" \
      python3 - <<'PY' || printf 'stage2 supervisor webhook post failed\n' >&2
import json
import os
import urllib.request

url = os.environ["STANDX_SUPERVISOR_WEBHOOK"]
text = os.environ["STANDX_STAGE2_NOTIFICATION"]
severity = os.environ["STANDX_STAGE2_NOTIFICATION_SEVERITY"]
webhook_format = os.environ.get("STANDX_SUPERVISOR_WEBHOOK_FORMAT", "slack").lower()
raw = {
    "text": text,
    "action": "stage2_ab_supervisor",
    "severity": severity,
}
if webhook_format in {"slack", "telegram"}:
    payload = {"text": text}
elif webhook_format == "feishu":
    payload = {"msg_type": "text", "content": {"text": text}}
elif webhook_format == "raw":
    payload = raw
else:
    raise SystemExit(f"unsupported STANDX_SUPERVISOR_WEBHOOK_FORMAT={webhook_format}")
request = urllib.request.Request(
    url,
    data=json.dumps(payload).encode("utf-8"),
    headers={"Content-Type": "application/json"},
    method="POST",
)
with urllib.request.urlopen(request, timeout=5) as response:
    if not 200 <= response.status < 300:
        raise RuntimeError(f"webhook returned HTTP {response.status}")
PY
  fi
}

critical_stop() {
  notify "CRITICAL stage2 A/B stopped: $1; no automatic flatten was attempted" critical
  exit 75
}

for value in "$poll_seconds"; do
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || {
    printf 'stage2 A/B durations must be positive integers\n' >&2
    exit 64
  }
done
[[ "$symbol" == "XAG-USD" ]] || {
  printf 'stage2 A/B is frozen to XAG-USD\n' >&2
  exit 64
}
[[ -x "$standx_bin" && -f "$baseline_config" && -f "$candidate_config" ]] || {
  printf 'stage2 A/B binary or frozen configs are missing\n' >&2
  exit 64
}

# Both configs must be byte-identical after normalizing the single enable line.
python3 - "$baseline_config" "$candidate_config" <<'PY' || exit 64
from pathlib import Path
import sys
baseline = Path(sys.argv[1]).read_text(encoding="utf-8")
candidate = Path(sys.argv[2]).read_text(encoding="utf-8")
if baseline.replace("enabled = false", "enabled = true") != candidate:
    raise SystemExit("stage2 arm configs differ outside adaptive_spread.enabled")
PY

if [[ "${STANDX_STAGE2_VALIDATE_ONLY:-0}" == "1" ]]; then
  printf 'stage2 A/B validation ok: symbol=%s baseline=%s candidate=%s\n' \
    "$symbol" "$(sha256sum "$baseline_config" | awk '{print $1}')" \
    "$(sha256sum "$candidate_config" | awk '{print $1}')"
  exit 0
fi

mkdir -p "$(dirname "$ab_lock")" "$log_dir"
exec 9>"$ab_lock"
flock -n 9 || {
  printf 'stage2 A/B orchestrator is already running or a normal live maker owns the guard\n' >&2
  exit 75
}
export STANDX_STAGE2_AB_MEMBER=1
export STANDX_STAGE2_AB_LOCK_PATH="$ab_lock"

positions_json() {
  "$standx_bin" --output json account positions --symbol "$symbol"
}

orders_json() {
  "$standx_bin" --output json account orders --symbol "$symbol"
}

json_array_empty() {
  python3 -c 'import json,sys
try:
    value = json.load(sys.stdin)
except (json.JSONDecodeError, UnicodeDecodeError):
    raise SystemExit(2)
if not isinstance(value, list):
    raise SystemExit(2)
raise SystemExit(0 if value == [] else 1)'
}

position_state() {
  local payload
  payload="$(positions_json)" || return 2
  printf '%s' "$payload" | json_array_empty
}

postcheck_is_empty() {
  orders_json | json_array_empty && position_state
}

manifest_position_is_flat() {
  python3 -c 'import json,sys; value=json.load(open(sys.argv[1], encoding="utf-8"))["log"].get("final_position"); raise SystemExit(0 if value is not None and abs(float(value)) <= 1e-12 else 1)' "$1"
}

postcheck_is_empty ||
  critical_stop "initial/restart preflight could not prove orders=[] and positions=[]"
notify "stage2 A/B preflight verified: orders=[] positions=[]"

run_arm() {
  local arm="$1"
  local config="$2"
  local config_hash run_id manifest pid status deadline grace_deadline position_status
  config_hash="$(sha256sum "$config" | awk '{print $1}')"
  run_id="stage2-${arm}-$(date -u +%Y%m%dT%H%M%SZ)-${config_hash:0:12}"
  manifest="$log_dir/$run_id.manifest.json"
  invalidate_arm() {
    python3 "$root/scripts/maker_run_manifest.py" invalidate \
      --manifest "$manifest" --reason "$1" >/dev/null 2>&1 || true
  }
  notify "stage2 A/B arm starting: arm=$arm run_id=$run_id config_hash=$config_hash"

  STANDX_RUN_ID="$run_id" "$root/scripts/run_maker_observed.sh" \
    "$standx_bin" --output json maker run "$symbol" --maker-config "$config" --live &
  pid=$!
  deadline=$((SECONDS + arm_seconds))
  while ((SECONDS < deadline)); do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid"
      status=$?
      invalidate_arm "arm exited before scheduled duration"
      critical_stop "arm=$arm exited before its 2-hour window (status=$status run_id=$run_id)"
    fi
    sleep "$poll_seconds"
  done

  grace_deadline=$((SECONDS + flat_grace_seconds))
  while true; do
    position_state
    position_status=$?
    if [[ "$position_status" == "0" ]]; then
      break
    fi
    if [[ "$position_status" != "1" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      invalidate_arm "venue position query or JSON validation failed"
      critical_stop "arm=$arm could not prove venue position state (run_id=$run_id)"
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid"
      status=$?
      invalidate_arm "arm exited while awaiting natural flat"
      critical_stop "arm=$arm exited while waiting for natural flat (status=$status run_id=$run_id)"
    fi
    if ((SECONDS >= grace_deadline)); then
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      invalidate_arm "position remained nonzero beyond flat grace"
      critical_stop "arm=$arm remained non-flat for ${flat_grace_seconds}s (run_id=$run_id invalid)"
    fi
    sleep "$poll_seconds"
  done

  kill -TERM "$pid" 2>/dev/null || true
  wait "$pid"
  status=$?
  if [[ "$status" != "0" ]]; then
    invalidate_arm "maker cleanup exited nonzero"
    critical_stop "arm=$arm cleanup exited $status (run_id=$run_id)"
  fi
  python3 "$root/scripts/maker_run_manifest.py" validate \
    --manifest "$manifest" --repo-root "$root" >/dev/null ||
    {
      invalidate_arm "manifest validation failed"
      critical_stop "arm=$arm manifest validation failed (run_id=$run_id)"
    }
  manifest_position_is_flat "$manifest" || {
    invalidate_arm "maker ledger manifest ended nonzero or unavailable"
    critical_stop "arm=$arm maker ledger manifest was not flat (run_id=$run_id)"
  }
  postcheck_is_empty || {
    invalidate_arm "independent post-check found orders or position"
    critical_stop "arm=$arm independent post-check found orders or position (run_id=$run_id)"
  }
  notify "stage2 A/B arm complete: arm=$arm run_id=$run_id config_hash=$config_hash orders=[] positions=[]"
}

while true; do
  run_arm baseline "$baseline_config"
  run_arm candidate "$candidate_config"
done
