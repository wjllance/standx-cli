#!/usr/bin/env python3
"""Offline acceptance verifier for maker strategy roadmap stage 1."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/standx"))
    parser.add_argument(
        "--trace", type=Path, default=Path("examples/maker-replay-trace.ndjson")
    )
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parent.parent)
    return parser.parse_args()


def replay(binary: Path, trace: Path, repo_root: Path) -> bytes:
    result = subprocess.run(
        [str(binary), "--output", "json", "maker", "replay", str(trace)],
        cwd=repo_root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"replay exited {result.returncode}: {result.stderr.decode(errors='replace')}"
        )
    if result.stderr:
        raise RuntimeError(f"replay wrote stderr: {result.stderr.decode(errors='replace')}")
    return result.stdout


def records(payload: bytes) -> list[dict[str, Any]]:
    parsed = [json.loads(line) for line in payload.splitlines() if line.strip()]
    if not all(isinstance(record, dict) for record in parsed):
        raise RuntimeError("replay output contains a non-object JSON record")
    return parsed


def close(actual: float, expected: float, tolerance: float = 1e-9) -> None:
    if not math.isclose(actual, expected, rel_tol=0.0, abs_tol=tolerance):
        raise RuntimeError(f"expected {expected}, got {actual}")


def verify_summary(summary: dict[str, Any]) -> dict[str, Any]:
    components = (
        float(summary["gross_spread_quote"])
        + float(summary["inventory_mtm_change_quote"])
        + float(summary["rebate_quote"])
        - float(summary["fee_quote"])
        + float(summary["funding_quote"])
        - float(summary["exit_cost_quote"])
    )
    net = float(summary["net_pnl_quote"])
    conservation_error = abs(components - net)
    if conservation_error > 0.01:
        raise RuntimeError(
            f"PnL conservation error {conservation_error} exceeds one 0.01 quote tick"
        )

    if summary["passive_fills"] != 1 or summary["exit_fills"] != 1:
        raise RuntimeError("fixture did not preserve independent passive/exit attribution")
    close(float(summary["passive_cashflow_quote"]), -99.95)
    close(float(summary["exit_cashflow_quote"]), 50.075)
    close(float(summary["passive_capture_bps"]), (100.0 - 99.95) / 99.95 * 10_000)
    close(float(summary["eligible_bid_qty_ms"]), 22_500.0)
    close(float(summary["eligible_ask_qty_ms"]), 47_500.0)
    close(float(summary["eligible_total_qty_ms"]), 70_000.0)
    close(float(summary["time_weighted_uptime_pct"]), 100.0)
    if summary["inventory_observed_ms"] != 35_000:
        raise RuntimeError("inventory observation interval is not 35000ms")
    if summary["inventory_nonzero_ms"] != 35_000:
        raise RuntimeError("inventory nonzero holding time is not 35000ms")
    close(float(summary["inventory_abs_qty_ms"]), 20_000.0)
    close(float(summary["inventory_avg_abs_qty"]), 4.0 / 7.0)

    markouts = {int(item["window_ms"]): item for item in summary["markouts"]}
    if set(markouts) != {1_000, 5_000, 30_000}:
        raise RuntimeError("missing required 1s/5s/30s markout windows")
    if markouts[30_000]["unavailable"] != 1 or markouts[30_000]["samples"] != 1:
        raise RuntimeError("30s missing horizon was not explicitly marked unavailable")
    if summary["execution_costs_unavailable"] != 0:
        raise RuntimeError("fixture contains an unexpectedly unavailable execution cost")
    if summary["funding_available"] is not True or summary["net_pnl_complete"] is not True:
        raise RuntimeError("fixture attribution is not marked complete despite explicit funding")
    return {
        "pnl_conservation_error": conservation_error,
        "passive_capture_bps": summary["passive_capture_bps"],
        "inventory_nonzero_ms": summary["inventory_nonzero_ms"],
        "inventory_abs_qty_ms": summary["inventory_abs_qty_ms"],
    }


def verify_core_purity(repo_root: Path) -> None:
    source = (repo_root / "crates/standx-maker/src/replay.rs").read_text(encoding="utf-8")
    banned = ("std::fs", "std::env", "Instant", "tokio::", "reqwest::", "println!")
    found = [token for token in banned if token in source]
    if found:
        raise RuntimeError(f"pure replay core contains forbidden runtime dependencies: {found}")


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    binary = args.binary if args.binary.is_absolute() else repo_root / args.binary
    trace = args.trace if args.trace.is_absolute() else repo_root / args.trace
    outputs = [replay(binary, trace, repo_root) for _ in range(3)]
    if outputs[0] != outputs[1] or outputs[1] != outputs[2]:
        raise RuntimeError("three replay outputs are not byte-identical")
    parsed = records(outputs[0])
    summaries = [record for record in parsed if record.get("action") == "replay_summary"]
    if len(summaries) != 1:
        raise RuntimeError(f"expected one replay_summary, got {len(summaries)}")
    details = verify_summary(summaries[0])
    verify_core_purity(repo_root)
    print(
        json.dumps(
            {
                "stage": 1,
                "status": "pass",
                "trace": str(trace.relative_to(repo_root)),
                "runs": 3,
                "output_sha256": hashlib.sha256(outputs[0]).hexdigest(),
                "byte_identical": True,
                "pure_replay_core": True,
                **details,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
