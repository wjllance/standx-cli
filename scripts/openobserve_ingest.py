#!/usr/bin/env python3
"""Validate and batch-upload immutable StandX NDJSON logs to OpenObserve."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import time
from typing import Any
from urllib import error, parse, request


SENSITIVE_KEYS = {
    "api_key",
    "api_secret",
    "authorization",
    "jwt",
    "password",
    "private_key",
    "secret",
    "token",
    "webhook",
}
STREAM_RE = re.compile(r"^[A-Za-z0-9_]+$")


def redact(value: Any) -> Any:
    if isinstance(value, dict):
        redacted: dict[str, Any] = {}
        for key, child in value.items():
            normalized = str(key).lower().replace("-", "_")
            if (
                normalized in SENSITIVE_KEYS
                or "webhook" in normalized
                or normalized.endswith("_token")
                or normalized.endswith("_secret")
                or normalized.endswith("_password")
                or normalized.endswith("_private_key")
            ):
                redacted[str(key)] = "[REDACTED]"
            else:
                redacted[str(key)] = redact(child)
        return redacted
    if isinstance(value, list):
        return [redact(item) for item in value]
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_state(path: Path) -> dict[str, int]:
    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"cannot read upload state {path}: {exc}") from exc
    if not isinstance(value, dict) or not all(
        isinstance(key, str) and isinstance(line, int) for key, line in value.items()
    ):
        raise RuntimeError(f"invalid upload state format in {path}")
    return value


def save_state(path: Path, state: dict[str, int]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(state, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    temporary.replace(path)


def post_batch(
    endpoint: str,
    username: str,
    password: str,
    events: list[dict[str, Any]],
    retries: int,
) -> None:
    credential = base64.b64encode(f"{username}:{password}".encode()).decode()
    body = json.dumps(events, separators=(",", ":")).encode()
    headers = {
        "Authorization": f"Basic {credential}",
        "Content-Type": "application/json",
        "User-Agent": "standx-openobserve-ingest/1",
    }
    last_error: Exception | None = None
    for attempt in range(retries + 1):
        try:
            req = request.Request(endpoint, data=body, headers=headers, method="POST")
            with request.urlopen(req, timeout=15) as response:
                if not 200 <= response.status < 300:
                    raise RuntimeError(f"OpenObserve returned HTTP {response.status}")
                return
        except (error.HTTPError, error.URLError, TimeoutError, RuntimeError) as exc:
            last_error = exc
            if attempt < retries:
                time.sleep(2**attempt)
    raise RuntimeError(f"OpenObserve upload failed after {retries + 1} attempt(s): {last_error}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate and upload StandX JSON-lines logs to OpenObserve"
    )
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="validate only")
    parser.add_argument("--force", action="store_true", help="ignore upload checkpoint")
    parser.add_argument("--batch-size", type=int, default=500)
    parser.add_argument("--retries", type=int, default=3)
    parser.add_argument("--run-id")
    parser.add_argument("--git-sha", default=os.getenv("STANDX_GIT_SHA", ""))
    parser.add_argument("--config-hash", default=os.getenv("STANDX_CONFIG_HASH", ""))
    parser.add_argument(
        "--state-file",
        type=Path,
        default=Path(os.getenv("OPENOBSERVE_STATE_FILE", "var/standx/openobserve-uploaded.json")),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.batch_size <= 0 or args.retries < 0:
        raise RuntimeError("batch size must be positive and retries must be non-negative")

    url = os.getenv("OPENOBSERVE_URL", "http://127.0.0.1:5080").rstrip("/")
    org = os.getenv("OPENOBSERVE_ORG", "default")
    stream = os.getenv("OPENOBSERVE_STREAM", "standx_maker")
    if not STREAM_RE.fullmatch(org) or not STREAM_RE.fullmatch(stream):
        raise RuntimeError("OpenObserve org and stream may contain only letters, digits, and underscore")

    username = os.getenv("OPENOBSERVE_USER", "")
    password = os.getenv("OPENOBSERVE_PASSWORD", "")
    if not args.dry_run and (not username or not password):
        raise RuntimeError("OPENOBSERVE_USER and OPENOBSERVE_PASSWORD are required")

    endpoint = (
        f"{url}/api/{parse.quote(org, safe='')}/{parse.quote(stream, safe='')}/_json"
    )
    state = {} if args.dry_run else load_state(args.state_file)
    summary = {"valid": 0, "invalid": 0, "uploaded": 0, "skipped": 0, "files": 0}

    for path in args.paths:
        if not path.is_file():
            raise RuntimeError(f"log file does not exist: {path}")
        summary["files"] += 1
        file_hash = sha256_file(path)
        state_key = hashlib.sha256(
            f"{url}|{org}|{stream}|{file_hash}".encode()
        ).hexdigest()
        checkpoint = 0 if args.force or args.dry_run else state.get(state_key, 0)
        run_id = args.run_id or path.stem
        batch: list[dict[str, Any]] = []
        last_processed = checkpoint

        with path.open("r", encoding="utf-8", errors="replace") as handle:
            for line_number, raw_line in enumerate(handle, start=1):
                if line_number <= checkpoint:
                    summary["skipped"] += 1
                    continue
                last_processed = line_number
                line = raw_line.strip()
                if not line:
                    summary["invalid"] += 1
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    summary["invalid"] += 1
                    continue
                if not isinstance(event, dict):
                    summary["invalid"] += 1
                    continue

                event = redact(event)
                event.setdefault("schema_version", "maker_event_v1")
                event.setdefault("service_name", "standx-cli")
                event.setdefault("run_id", run_id)
                event.setdefault("source_file", path.name)
                event.setdefault(
                    "event_id",
                    hashlib.sha256(f"{file_hash}:{line_number}".encode()).hexdigest(),
                )
                if "ts" in event:
                    event.setdefault("_timestamp", event["ts"])
                if args.git_sha:
                    event.setdefault("git_sha", args.git_sha)
                if args.config_hash:
                    event.setdefault("config_hash", args.config_hash)
                batch.append(event)
                summary["valid"] += 1

                if len(batch) >= args.batch_size:
                    if not args.dry_run:
                        post_batch(endpoint, username, password, batch, args.retries)
                        summary["uploaded"] += len(batch)
                        state[state_key] = last_processed
                        save_state(args.state_file, state)
                    batch.clear()

        if batch and not args.dry_run:
            post_batch(endpoint, username, password, batch, args.retries)
            summary["uploaded"] += len(batch)
        if not args.dry_run:
            state[state_key] = last_processed
            save_state(args.state_file, state)

    print(json.dumps(summary, sort_keys=True))
    if summary["valid"] == 0 and summary["skipped"] == 0:
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as exc:
        print(f"openobserve ingest error: {exc}", file=sys.stderr)
        raise SystemExit(1)
