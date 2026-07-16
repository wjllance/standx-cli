#!/usr/bin/env python3
"""Create and finalize a reproducible maker-run sidecar manifest.

The manifest is intentionally separate from maker stdout so baseline identity
can improve without changing the established maker JSON event contract.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
from typing import Any, Sequence


SCHEMA_VERSION = "maker_baseline_manifest_v1"
STRATEGY_SOURCE_PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/standx-maker",
    "crates/standx-sdk",
    "crates/standx-cli",
    "examples/maker.toml",
)
VALUE_OVERRIDES = {
    "--spread-bps",
    "--band-bps",
    "--size",
    "--levels",
    "--level-step-bps",
    "--refresh-bps",
    "--interval",
    "-i",
    "--max-position",
    "--skew-bps",
    "--inventory-exit-pct",
    "--inventory-exit-qty",
    "--max-divergence-bps",
    "--vol-pause-bps",
    "--vol-window",
    "--adaptive-spread",
    "--stop-loss",
    "--alert-loss",
    "--alert-inventory-pct",
    "--alert-position-change-pct",
    "--alert-uptime",
    "--alert-equity-below",
    "--alert-margin-below",
    "--order-response-reconnect-attempts",
    "--order-response-reconnect-backoff",
    "--account-stream-reconnect-attempts",
    "--account-stream-reconnect-backoff",
    "--recovery-incidents-per-window",
    "--recovery-window-secs",
}
BOOLEAN_OVERRIDES = {"--no-ws", "--adaptive-spread"}
REQUIRED_CHECKS = {
    "git_sha_present",
    "strategy_source_clean",
    "program_hash_present",
    "collector_hashes_present",
    "config_hash_present",
    "symbol_present",
    "symbol_matches_events",
    "symbol_metadata_complete",
    "time_range_present",
    "timestamps_monotonic",
    "json_only",
    "cycle_sequence_complete",
    "lifecycle_started",
    "lifecycle_stopped",
}


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_sha(repo_root: Path) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    value = result.stdout.strip()
    return value if result.returncode == 0 and len(value) == 40 else None


def git_dirty_paths(repo_root: Path, paths: Sequence[str] = ()) -> list[str] | None:
    command = ["git", "-C", str(repo_root), "status", "--porcelain", "--untracked-files=all"]
    if paths:
        command.extend(["--", *paths])
    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    return [line[3:] for line in result.stdout.splitlines() if len(line) > 3]


def program_identity(command: Sequence[str], repo_root: Path) -> dict[str, str | None]:
    if not command:
        return {"path": None, "sha256": None}
    executable = command[0]
    located = shutil.which(executable)
    path = Path(located) if located else Path(executable)
    if not path.is_absolute():
        path = repo_root / path
    resolved = path.resolve()
    return {
        "path": display_path(resolved, repo_root),
        "sha256": sha256_file(resolved) if resolved.is_file() else None,
    }


def collector_identity(wrapper: Path | None, repo_root: Path) -> dict[str, Any]:
    tool = Path(__file__).resolve()
    wrapper_path = wrapper.resolve() if wrapper else None
    return {
        "manifest_tool": {
            "path": display_path(tool, repo_root),
            "sha256": sha256_file(tool),
        },
        "wrapper": {
            "path": display_path(wrapper_path, repo_root) if wrapper_path else None,
            "sha256": sha256_file(wrapper_path)
            if wrapper_path and wrapper_path.is_file()
            else None,
        },
    }


def maker_symbol(command: Sequence[str]) -> str | None:
    for index in range(len(command) - 2):
        if command[index] == "maker" and command[index + 1] == "run":
            candidate = command[index + 2]
            return candidate if not candidate.startswith("-") else None
    return None


def strategy_overrides(command: Sequence[str]) -> dict[str, Any]:
    """Return only known non-sensitive strategy flags from a maker command."""
    result: dict[str, Any] = {}
    index = 0
    while index < len(command):
        arg = command[index]
        if arg in BOOLEAN_OVERRIDES:
            result[arg.removeprefix("--").replace("-", "_")] = True
        elif arg in VALUE_OVERRIDES and index + 1 < len(command):
            key = "interval" if arg == "-i" else arg.removeprefix("--").replace("-", "_")
            result[key] = command[index + 1]
            index += 1
        elif "=" in arg:
            name, value = arg.split("=", 1)
            if name in VALUE_OVERRIDES:
                result[name.removeprefix("--").replace("-", "_")] = value
        index += 1
    return result


def display_path(path: Path, repo_root: Path) -> str:
    resolved = path.resolve()
    try:
        return str(resolved.relative_to(repo_root.resolve()))
    except ValueError:
        return str(resolved)


def atomic_write(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def parse_event_timestamp(value: Any) -> tuple[str, float] | None:
    try:
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            parsed = dt.datetime.fromtimestamp(value, dt.timezone.utc)
        elif isinstance(value, str) and value:
            parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
            if parsed.tzinfo is None:
                return None
            parsed = parsed.astimezone(dt.timezone.utc)
        else:
            return None
    except (OverflowError, OSError, ValueError):
        return None
    return parsed.isoformat().replace("+00:00", "Z"), parsed.timestamp()


def finite_float(value: Any) -> float | None:
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    if math.isfinite(parsed):
        return parsed
    return None


def classify_regime(metrics: dict[str, Any], cycles: int, duration_seconds: float | None) -> str:
    if cycles < 30 or duration_seconds is None or duration_seconds < 30:
        return "insufficient_window"
    range_bps = metrics.get("range_bps")
    net_move_bps = metrics.get("net_move_bps")
    directionality = metrics.get("directionality")
    max_vol_bps = metrics.get("max_vol_bps")
    halted_cycles = metrics.get("halted_cycles", 0)
    if max_vol_bps is not None and max_vol_bps >= 50:
        return "fast_or_stressed"
    if halted_cycles > 0 and range_bps is not None and range_bps >= 50:
        return "fast_or_stressed"
    if (
        net_move_bps is not None
        and abs(net_move_bps) >= 75
        and directionality is not None
        and directionality >= 0.7
        and halted_cycles == 0
    ):
        return "trend"
    if (
        range_bps is not None
        and range_bps <= 10
        and net_move_bps is not None
        and abs(net_move_bps) <= 5
    ):
        return "calm"
    return "unclassified"


def analyze_log(path: Path) -> dict[str, Any]:
    cycles: set[int] = set()
    cycle_counts: dict[int, int] = {}
    observed_cycles: set[int] = set()
    lifecycle_events: list[str] = []
    symbols: set[str] = set()
    market_sources: set[str] = set()
    first_timestamp: str | None = None
    last_timestamp: str | None = None
    first_timestamp_epoch: float | None = None
    last_timestamp_epoch: float | None = None
    prior_timestamp_epoch: float | None = None
    timestamp_regressions = 0
    marks: list[float] = []
    uptime_values: list[float] = []
    max_fills_total = 0
    halted_cycles = 0
    fallback_cycles = 0
    max_vol_bps: float | None = None
    final_position: float | None = None
    market_source_counts: dict[str, int] = {}
    fallback_reason_counts: dict[str, int] = {}
    json_lines = 0
    invalid_lines = 0

    with path.open(encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                invalid_lines += 1
                continue
            if not isinstance(event, dict):
                invalid_lines += 1
                continue
            json_lines += 1
            timestamp = parse_event_timestamp(event.get("ts"))
            if timestamp is not None:
                formatted_timestamp, timestamp_epoch = timestamp
                first_timestamp = first_timestamp or formatted_timestamp
                first_timestamp_epoch = (
                    first_timestamp_epoch if first_timestamp_epoch is not None else timestamp_epoch
                )
                last_timestamp = formatted_timestamp
                last_timestamp_epoch = timestamp_epoch
                if prior_timestamp_epoch is not None and timestamp_epoch < prior_timestamp_epoch:
                    timestamp_regressions += 1
                prior_timestamp_epoch = timestamp_epoch
            symbol = event.get("symbol")
            if isinstance(symbol, str) and symbol:
                symbols.add(symbol)
            source = event.get("market_source")
            if isinstance(source, str) and source:
                market_sources.add(source)
                market_source_counts[source] = market_source_counts.get(source, 0) + 1
            if event.get("action") in {"cycle_summary", "skip"} and isinstance(
                event.get("cycle"), int
            ):
                observed_cycle = event["cycle"]
                observed_cycles.add(observed_cycle)
            if event.get("action") == "cycle_summary" and isinstance(event.get("cycle"), int):
                cycle = event["cycle"]
                cycles.add(cycle)
                cycle_counts[cycle] = cycle_counts.get(cycle, 0) + 1
                mark = finite_float(event.get("mark"))
                if mark is not None and mark > 0:
                    marks.append(mark)
                uptime = finite_float(event.get("uptime_pct"))
                if uptime is not None:
                    uptime_values.append(uptime)
                fills_total = event.get("fills_total")
                if isinstance(fills_total, int):
                    max_fills_total = max(max_fills_total, fills_total)
                if event.get("halted") is True:
                    halted_cycles += 1
                fallback_reason = event.get("market_fallback_reason")
                if isinstance(fallback_reason, str) and fallback_reason:
                    fallback_cycles += 1
                    fallback_reason_counts[fallback_reason] = (
                        fallback_reason_counts.get(fallback_reason, 0) + 1
                    )
                vol_bps = finite_float(event.get("rolling_vol_bps"))
                if vol_bps is None:
                    vol_bps = finite_float(event.get("vol_bps"))
                if vol_bps is not None:
                    max_vol_bps = vol_bps if max_vol_bps is None else max(max_vol_bps, vol_bps)
                position = finite_float(event.get("position"))
                if position is not None:
                    final_position = position
            if event.get("action") == "lifecycle" and isinstance(event.get("event"), str):
                lifecycle_events.append(event["event"])
            if event.get("action") == "performance_summary":
                position = finite_float(event.get("position"))
                if position is not None:
                    final_position = position

    cycle_min = min(observed_cycles) if observed_cycles else None
    cycle_max = max(observed_cycles) if observed_cycles else None
    missing_cycles = (
        sorted(set(range(cycle_min, cycle_max + 1)) - observed_cycles)
        if cycle_min is not None and cycle_max is not None
        else []
    )
    duplicate_cycles = sorted(cycle for cycle, count in cycle_counts.items() if count > 1)
    duration_seconds = (
        last_timestamp_epoch - first_timestamp_epoch
        if last_timestamp_epoch is not None and first_timestamp_epoch is not None
        else None
    )
    start_mark = marks[0] if marks else None
    end_mark = marks[-1] if marks else None
    min_mark = min(marks) if marks else None
    max_mark = max(marks) if marks else None
    net_move_bps = (
        (end_mark / start_mark - 1) * 10_000
        if start_mark is not None and end_mark is not None
        else None
    )
    range_bps = (
        (max_mark - min_mark) / min_mark * 10_000
        if min_mark is not None and max_mark is not None
        else None
    )
    directionality = (
        abs(net_move_bps) / range_bps
        if net_move_bps is not None and range_bps is not None and range_bps > 0
        else None
    )
    regime_metrics = {
        "start_mark": start_mark,
        "end_mark": end_mark,
        "min_mark": min_mark,
        "max_mark": max_mark,
        "net_move_bps": net_move_bps,
        "range_bps": range_bps,
        "directionality": directionality,
        "halted_cycles": halted_cycles,
        "fallback_cycles": fallback_cycles,
        "max_vol_bps": max_vol_bps,
        "avg_uptime_pct": sum(uptime_values) / len(uptime_values) if uptime_values else None,
        "fills_total": max_fills_total,
    }
    regime = classify_regime(regime_metrics, len(cycles), duration_seconds)
    return {
        "sha256": sha256_file(path),
        "bytes": path.stat().st_size,
        "json_lines": json_lines,
        "invalid_lines": invalid_lines,
        "first_event_at": first_timestamp,
        "last_event_at": last_timestamp,
        "duration_seconds": duration_seconds,
        "timestamp_regressions": timestamp_regressions,
        "symbols": sorted(symbols),
        "market_sources": sorted(market_sources),
        "market_source_counts": market_source_counts,
        "fallback_reason_counts": fallback_reason_counts,
        "cycle_summaries": len(cycles),
        "cycle_summary_lines": sum(cycle_counts.values()),
        "cycle_min": cycle_min,
        "cycle_max": cycle_max,
        "missing_cycles": missing_cycles,
        "duplicate_cycles": duplicate_cycles,
        "lifecycle_events": lifecycle_events,
        "regime": regime,
        "regime_metrics": regime_metrics,
        "comparison_window_eligible": len(cycles) >= 300
        and duration_seconds is not None
        and duration_seconds >= 600,
        "final_position": final_position,
    }


def start_manifest(args: argparse.Namespace) -> int:
    repo_root = args.repo_root.resolve()
    config_file = args.config_file.resolve() if args.config_file else None
    command = list(args.command)
    if command and command[0] == "--":
        command = command[1:]
    symbol = maker_symbol(command)
    worktree_dirty_paths = git_dirty_paths(repo_root)
    strategy_dirty_paths = git_dirty_paths(repo_root, STRATEGY_SOURCE_PATHS)
    payload: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "status": "running",
        "run_id": args.run_id,
        "started_at": utc_now(),
        "git_sha": git_sha(repo_root),
        "git_dirty": bool(worktree_dirty_paths) if worktree_dirty_paths is not None else None,
        "git_dirty_paths": worktree_dirty_paths,
        "strategy_source_paths": list(STRATEGY_SOURCE_PATHS),
        "strategy_dirty_paths": strategy_dirty_paths,
        "symbol": symbol,
        "mode": "live" if "--live" in command else "paper",
        "program": program_identity(command, repo_root),
        "collector": collector_identity(args.collector_wrapper, repo_root),
        "config": {
            "path": display_path(config_file, repo_root) if config_file else None,
            "sha256": sha256_file(config_file) if config_file and config_file.is_file() else None,
            "strategy_overrides": strategy_overrides(command),
        },
        "symbol_metadata": {
            "price_tick_decimals": args.price_tick_decimals,
            "qty_tick_decimals": args.qty_tick_decimals,
            "min_order_qty": args.min_order_qty,
        },
        "log": {"path": display_path(args.log, repo_root)},
    }
    atomic_write(args.manifest, payload)
    return 0


def finalize_manifest(args: argparse.Namespace) -> int:
    payload = json.loads(args.manifest.read_text(encoding="utf-8"))
    log = analyze_log(args.log)
    declared_symbol = payload.get("symbol")
    symbol_match = not log["symbols"] or log["symbols"] == [declared_symbol]
    metadata = payload.get("symbol_metadata", {})
    collector = payload.get("collector", {})
    checks = {
        "git_sha_present": isinstance(payload.get("git_sha"), str) and len(payload["git_sha"]) == 40,
        "strategy_source_clean": payload.get("strategy_dirty_paths") == [],
        "program_hash_present": isinstance(payload.get("program", {}).get("sha256"), str),
        "collector_hashes_present": isinstance(
            collector.get("manifest_tool", {}).get("sha256"), str
        )
        and isinstance(collector.get("wrapper", {}).get("sha256"), str),
        "config_hash_present": isinstance(payload.get("config", {}).get("sha256"), str),
        "symbol_present": isinstance(declared_symbol, str) and bool(declared_symbol),
        "symbol_matches_events": symbol_match,
        "symbol_metadata_complete": all(metadata.get(key) is not None for key in metadata),
        "time_range_present": bool(log["first_event_at"] and log["last_event_at"]),
        "timestamps_monotonic": log["timestamp_regressions"] == 0,
        "json_only": log["invalid_lines"] == 0,
        "cycle_sequence_complete": bool(log["cycle_summaries"])
        and not log["missing_cycles"]
        and not log["duplicate_cycles"],
        "lifecycle_started": "started" in log["lifecycle_events"],
        "lifecycle_stopped": "stopped" in log["lifecycle_events"],
    }
    payload.update(
        {
            "status": "finished",
            "completed_at": utc_now(),
            "exit_status": args.exit_status,
            "log": {**payload.get("log", {}), **log},
            "validation": {
                "checks": checks,
                "baseline_eligible": args.exit_status == 0 and all(checks.values()),
            },
        }
    )
    atomic_write(args.manifest, payload)
    return 0


def invalidate_manifest(args: argparse.Namespace) -> int:
    payload = json.loads(args.manifest.read_text(encoding="utf-8"))
    validation = payload.setdefault("validation", {})
    validation["baseline_eligible"] = False
    reasons = validation.setdefault("invalid_reasons", [])
    if args.reason not in reasons:
        reasons.append(args.reason)
    payload["status"] = "invalid"
    payload["invalidated_at"] = utc_now()
    atomic_write(args.manifest, payload)
    return 0


def validate_manifest(args: argparse.Namespace) -> int:
    payload = json.loads(args.manifest.read_text(encoding="utf-8"))
    errors: list[str] = []
    if payload.get("schema_version") != SCHEMA_VERSION:
        errors.append("unsupported schema_version")
    validation = payload.get("validation", {})
    checks = validation.get("checks", {})
    missing_checks = REQUIRED_CHECKS - set(checks)
    errors.extend(f"missing check: {name}" for name in sorted(missing_checks))
    errors.extend(f"failed check: {name}" for name, passed in checks.items() if not passed)
    if not validation.get("baseline_eligible"):
        errors.append("manifest is not baseline eligible")

    stored_path = payload.get("log", {}).get("path")
    if not isinstance(stored_path, str) or not stored_path:
        errors.append("log path is missing")
    else:
        log_path = Path(stored_path)
        if not log_path.is_absolute():
            log_path = args.repo_root / log_path
        if not log_path.is_file():
            errors.append(f"log file is missing: {log_path}")
        else:
            expected_hash = payload.get("log", {}).get("sha256")
            actual_hash = sha256_file(log_path)
            if expected_hash != actual_hash:
                errors.append("log SHA-256 mismatch")

    result = {
        "manifest": str(args.manifest),
        "valid": not errors,
        "errors": errors,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if not errors else 1


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    subparsers = root.add_subparsers(dest="operation", required=True)

    start = subparsers.add_parser("start")
    start.add_argument("--manifest", type=Path, required=True)
    start.add_argument("--log", type=Path, required=True)
    start.add_argument("--run-id", required=True)
    start.add_argument("--repo-root", type=Path, required=True)
    start.add_argument("--config-file", type=Path)
    start.add_argument("--collector-wrapper", type=Path)
    start.add_argument("--price-tick-decimals", type=int)
    start.add_argument("--qty-tick-decimals", type=int)
    start.add_argument("--min-order-qty")
    start.add_argument("command", nargs=argparse.REMAINDER)
    start.set_defaults(handler=start_manifest)

    finalize = subparsers.add_parser("finalize")
    finalize.add_argument("--manifest", type=Path, required=True)
    finalize.add_argument("--log", type=Path, required=True)
    finalize.add_argument("--exit-status", type=int, required=True)
    finalize.set_defaults(handler=finalize_manifest)

    invalidate = subparsers.add_parser("invalidate")
    invalidate.add_argument("--manifest", type=Path, required=True)
    invalidate.add_argument("--reason", required=True)
    invalidate.set_defaults(handler=invalidate_manifest)

    validate = subparsers.add_parser("validate")
    validate.add_argument("--manifest", type=Path, required=True)
    validate.add_argument("--repo-root", type=Path, default=Path.cwd())
    validate.set_defaults(handler=validate_manifest)
    return root


def main() -> int:
    args = parser().parse_args()
    return args.handler(args)


if __name__ == "__main__":
    raise SystemExit(main())
