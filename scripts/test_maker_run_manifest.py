#!/usr/bin/env python3
"""Tests for maker baseline sidecar manifests."""

from __future__ import annotations

import argparse
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import maker_run_manifest as manifest


class MakerRunManifestTests(unittest.TestCase):
    def test_start_records_only_non_sensitive_strategy_overrides(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "maker.toml"
            config.write_text("spread_bps = 8\n", encoding="utf-8")
            output = root / "run.manifest.json"
            args = argparse.Namespace(
                manifest=output,
                log=root / "run.ndjson",
                run_id="test-run",
                repo_root=root,
                config_file=config,
                collector_wrapper=None,
                price_tick_decimals=2,
                qty_tick_decimals=4,
                min_order_qty="0.0001",
                command=[
                    "standx",
                    "--output",
                    "json",
                    "maker",
                    "run",
                    "BTC-USD",
                    "--spread-bps",
                    "7",
                    "--alert-webhook",
                    "https://secret.example/token",
                    "--live",
                ],
            )
            with (
                mock.patch.object(manifest, "git_sha", return_value="a" * 40),
                mock.patch.object(manifest, "git_dirty_paths", return_value=[]),
            ):
                manifest.start_manifest(args)

            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(payload["symbol"], "BTC-USD")
            self.assertEqual(payload["mode"], "live")
            self.assertEqual(payload["config"]["strategy_overrides"], {"spread_bps": "7"})
            self.assertNotIn("secret.example", output.read_text(encoding="utf-8"))

    def test_finalize_marks_complete_trace_as_eligible(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "run.manifest.json"
            log = root / "run.ndjson"
            output.write_text(
                json.dumps(
                    {
                        "schema_version": manifest.SCHEMA_VERSION,
                        "status": "running",
                        "git_sha": "a" * 40,
                        "git_dirty": False,
                        "strategy_dirty_paths": [],
                        "symbol": "BTC-USD",
                        "program": {"sha256": "c" * 64},
                        "collector": {
                            "manifest_tool": {"sha256": "d" * 64},
                            "wrapper": {"sha256": "e" * 64},
                        },
                        "config": {"sha256": "b" * 64},
                        "symbol_metadata": {
                            "price_tick_decimals": 2,
                            "qty_tick_decimals": 4,
                            "min_order_qty": "0.0001",
                        },
                        "log": {"path": "run.ndjson"},
                    }
                ),
                encoding="utf-8",
            )
            log.write_text(
                '{"ts":"2026-07-15T00:00:00Z","symbol":"BTC-USD","action":"lifecycle","event":"started"}\n'
                '{"ts":"2026-07-15T00:00:01Z","symbol":"BTC-USD","action":"cycle_summary","cycle":0,"market_source":"ws"}\n'
                '{"ts":"2026-07-15T00:00:02Z","symbol":"BTC-USD","action":"lifecycle","event":"stopped"}\n',
                encoding="utf-8",
            )
            manifest.finalize_manifest(
                argparse.Namespace(manifest=output, log=log, exit_status=0)
            )

            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertTrue(payload["validation"]["baseline_eligible"])
            self.assertEqual(payload["log"]["cycle_summaries"], 1)
            self.assertEqual(payload["log"]["market_sources"], ["ws"])
            self.assertEqual(payload["log"]["regime"], "insufficient_window")

            with contextlib.redirect_stdout(io.StringIO()):
                result = manifest.validate_manifest(
                    argparse.Namespace(manifest=output, repo_root=root)
                )
            self.assertEqual(result, 0)

            with log.open("a", encoding="utf-8") as handle:
                handle.write('{"action":"tampered"}\n')
            with contextlib.redirect_stdout(io.StringIO()):
                result = manifest.validate_manifest(
                    argparse.Namespace(manifest=output, repo_root=root)
                )
            self.assertEqual(result, 1)

    def test_finalize_preserves_incomplete_trace_reasons(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "run.manifest.json"
            log = root / "run.ndjson"
            output.write_text(
                json.dumps(
                    {
                        "git_sha": "a" * 40,
                        "git_dirty": False,
                        "strategy_dirty_paths": [],
                        "symbol": "BTC-USD",
                        "program": {"sha256": "c" * 64},
                        "collector": {
                            "manifest_tool": {"sha256": "d" * 64},
                            "wrapper": {"sha256": "e" * 64},
                        },
                        "config": {"sha256": "b" * 64},
                        "symbol_metadata": {
                            "price_tick_decimals": None,
                            "qty_tick_decimals": None,
                            "min_order_qty": None,
                        },
                        "log": {},
                    }
                ),
                encoding="utf-8",
            )
            log.write_text(
                '{"ts":1,"symbol":"BTC-USD","action":"cycle_summary","cycle":0}\n'
                '{"ts":2,"symbol":"BTC-USD","action":"cycle_summary","cycle":2}\n'
                '{"ts":2,"symbol":"BTC-USD","action":"cycle_summary","cycle":2}\n',
                encoding="utf-8",
            )
            manifest.finalize_manifest(
                argparse.Namespace(manifest=output, log=log, exit_status=0)
            )

            payload = json.loads(output.read_text(encoding="utf-8"))
            checks = payload["validation"]["checks"]
            self.assertFalse(payload["validation"]["baseline_eligible"])
            self.assertFalse(checks["symbol_metadata_complete"])
            self.assertFalse(checks["cycle_sequence_complete"])
            self.assertFalse(checks["lifecycle_stopped"])
            self.assertEqual(payload["log"]["missing_cycles"], [1])
            self.assertEqual(payload["log"]["duplicate_cycles"], [2])

    def test_regime_report_distinguishes_calm_trend_and_stress(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cases = {
                "calm": [100.0 + (index % 2) * 0.02 for index in range(30)],
                "trend": [100.0 + index * 0.01 for index in range(100)],
                "fast_or_stressed": [100.0 + (2.0 if index == 15 else 0.0) for index in range(30)],
            }
            for expected, marks in cases.items():
                log = root / f"{expected}.ndjson"
                events = []
                for index, mark in enumerate(marks):
                    event = {
                        "ts": index * 2,
                        "symbol": "TEST-USD",
                        "action": "cycle_summary",
                        "cycle": index,
                        "mark": mark,
                        "uptime_pct": 100.0,
                        "fills_total": 0,
                        "halted": expected == "fast_or_stressed" and index == 15,
                        "vol_bps": 200.0 if expected == "fast_or_stressed" and index == 15 else None,
                    }
                    events.append(json.dumps(event))
                log.write_text("\n".join(events) + "\n", encoding="utf-8")
                self.assertEqual(manifest.analyze_log(log)["regime"], expected)


if __name__ == "__main__":
    unittest.main()
