#!/usr/bin/env python3
"""Focused tests for incremental OpenObserve ingestion."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import openobserve_ingest as ingest


class IncrementalIngestTests(unittest.TestCase):
    def args(self, path: Path, state_file: Path) -> argparse.Namespace:
        return argparse.Namespace(
            paths=[path],
            run_id="test-live-run",
            incremental=True,
            force=False,
            dry_run=False,
            batch_size=500,
            retries=0,
            git_sha="abc1234",
            config_hash="config123",
            state_file=state_file,
        )

    def upload(self, args: argparse.Namespace, state: dict[str, int]) -> dict[str, int]:
        return ingest.upload_once(
            args,
            url="http://openobserve.test:5080",
            org="default",
            stream="standx_maker",
            endpoint="http://openobserve.test:5080/api/default/standx_maker/_json",
            username="user",
            password="password",
            state=state,
        )

    def test_growing_file_uploads_only_new_lines_with_stable_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "run.ndjson"
            state_file = root / "state.json"
            log.write_text(
                '{"action":"lifecycle","event":"started"}\n'
                '{"action":"cycle_summary","cycle":1}\n',
                encoding="utf-8",
            )
            args = self.args(log, state_file)
            state: dict[str, int] = {}
            posted: list[dict[str, object]] = []

            with mock.patch.object(
                ingest,
                "post_batch",
                side_effect=lambda _endpoint, _user, _password, events, _retries: posted.extend(events),
            ):
                first = self.upload(args, state)
                first_ids = [event["event_id"] for event in posted]
                with log.open("a", encoding="utf-8") as handle:
                    handle.write('{"action":"cycle_summary","cycle":2}\n')
                second = self.upload(args, state)

            self.assertEqual(first["uploaded"], 2)
            self.assertEqual(second["uploaded"], 1)
            self.assertEqual(second["skipped"], 2)
            self.assertEqual(len(posted), 3)
            self.assertEqual(first_ids[0], ingest.incremental_event_id("test-live-run", 1))
            self.assertEqual(first_ids[1], ingest.incremental_event_id("test-live-run", 2))
            self.assertEqual(posted[2]["event_id"], ingest.incremental_event_id("test-live-run", 3))
            self.assertEqual(len({event["event_id"] for event in posted}), 3)

    def test_partial_trailing_line_is_not_checkpointed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "run.ndjson"
            state_file = root / "state.json"
            log.write_text(
                '{"action":"cycle_summary","cycle":1}\n{"action":"cycle_',
                encoding="utf-8",
            )
            args = self.args(log, state_file)
            state: dict[str, int] = {}
            posted: list[dict[str, object]] = []

            with mock.patch.object(
                ingest,
                "post_batch",
                side_effect=lambda _endpoint, _user, _password, events, _retries: posted.extend(events),
            ):
                first = self.upload(args, state)
                with log.open("a", encoding="utf-8") as handle:
                    handle.write('summary","cycle":2}\n')
                second = self.upload(args, state)

            key = ingest.incremental_state_key(
                "http://openobserve.test:5080", "default", "standx_maker", "test-live-run"
            )
            self.assertEqual(first["uploaded"], 1)
            self.assertEqual(second["uploaded"], 1)
            self.assertEqual(state[key], 2)
            self.assertEqual([event["cycle"] for event in posted], [1, 2])

    def test_failed_upload_does_not_advance_checkpoint(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "run.ndjson"
            state_file = root / "state.json"
            log.write_text(json.dumps({"action": "cycle_summary", "cycle": 1}) + "\n")
            args = self.args(log, state_file)
            state: dict[str, int] = {}

            with mock.patch.object(
                ingest, "post_batch", side_effect=RuntimeError("temporary outage")
            ):
                with self.assertRaisesRegex(RuntimeError, "temporary outage"):
                    self.upload(args, state)

            self.assertEqual(state, {})
            self.assertFalse(state_file.exists())


if __name__ == "__main__":
    unittest.main()
