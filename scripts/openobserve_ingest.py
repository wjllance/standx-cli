#!/usr/bin/env python3
"""Validate and batch-upload StandX NDJSON logs to OpenObserve."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import signal
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
STOP_REQUESTED = False


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


def preflight(
    url: str,
    org: str,
    stream: str,
    username: str,
    password: str,
) -> None:
    """Verify that the configured credentials can query the target stream."""
    now_us = int(time.time() * 1_000_000)
    body = json.dumps(
        {
            "query": {
                "sql": f'SELECT count(*) AS events FROM "{stream}"',
                "start_time": now_us - 60 * 1_000_000,
                "end_time": now_us,
                "from": 0,
                "size": 1,
            }
        }
    ).encode()
    credential = base64.b64encode(f"{username}:{password}".encode()).decode()
    endpoint = f"{url}/api/{parse.quote(org, safe='')}/_search?type=logs"
    req = request.Request(
        endpoint,
        data=body,
        headers={
            "Authorization": f"Basic {credential}",
            "Content-Type": "application/json",
            "User-Agent": "standx-openobserve-ingest/1",
        },
        method="POST",
    )
    try:
        with request.urlopen(req, timeout=15) as response:
            if not 200 <= response.status < 300:
                raise RuntimeError(f"OpenObserve returned HTTP {response.status}")
    except error.HTTPError as exc:
        detail = exc.read(2048).decode(errors="replace")
        raise RuntimeError(
            f"OpenObserve preflight returned HTTP {exc.code}: {detail}"
        ) from exc
    except (error.URLError, TimeoutError) as exc:
        raise RuntimeError(f"OpenObserve preflight failed: {exc}") from exc


def incremental_state_key(url: str, org: str, stream: str, run_id: str) -> str:
    return hashlib.sha256(
        f"incremental-v1|{url}|{org}|{stream}|{run_id}".encode()
    ).hexdigest()


def incremental_event_id(run_id: str, line_number: int) -> str:
    return hashlib.sha256(
        f"incremental-v1|{run_id}|{line_number}".encode()
    ).hexdigest()


def merge_summary(target: dict[str, int], source: dict[str, int]) -> None:
    # skipped/files are per-poll scan diagnostics and would be misleading when
    # summed across a long-running follower.
    for key in ("valid", "invalid", "uploaded"):
        target[key] += source[key]


def upload_once(
    args: argparse.Namespace,
    *,
    url: str,
    org: str,
    stream: str,
    endpoint: str,
    username: str,
    password: str,
    state: dict[str, int],
) -> dict[str, int]:
    summary = {"valid": 0, "invalid": 0, "uploaded": 0, "skipped": 0, "files": 0}

    for path in args.paths:
        if not path.is_file():
            raise RuntimeError(f"log file does not exist: {path}")
        summary["files"] += 1
        run_id = args.run_id or path.stem
        file_hash = ""
        if args.incremental:
            state_key = incremental_state_key(url, org, stream, run_id)
        else:
            file_hash = sha256_file(path)
            state_key = hashlib.sha256(
                f"{url}|{org}|{stream}|{file_hash}".encode()
            ).hexdigest()
        checkpoint = 0 if args.force or args.dry_run else state.get(state_key, 0)
        batch: list[dict[str, Any]] = []
        last_processed = checkpoint

        with path.open("r", encoding="utf-8", errors="replace") as handle:
            for line_number, raw_line in enumerate(handle, start=1):
                if line_number <= checkpoint:
                    summary["skipped"] += 1
                    continue
                # A growing file may be observed between write() and its trailing newline.
                # Leave that line uncheckpointed so the next pass can parse it completely.
                if args.incremental and not raw_line.endswith("\n"):
                    break
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
                if args.incremental:
                    event.setdefault("event_id", incremental_event_id(run_id, line_number))
                else:
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

    return summary


def request_stop(_signum: int, _frame: Any) -> None:
    global STOP_REQUESTED
    STOP_REQUESTED = True


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate and upload StandX JSON-lines logs to OpenObserve"
    )
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="validate only")
    parser.add_argument("--force", action="store_true", help="ignore upload checkpoint")
    parser.add_argument(
        "--incremental",
        action="store_true",
        help="use stable run/line checkpoints for a file that may still be growing",
    )
    parser.add_argument(
        "--follow",
        action="store_true",
        help="keep uploading appended lines until SIGINT or SIGTERM",
    )
    parser.add_argument(
        "--preflight",
        action="store_true",
        help="verify OpenObserve connectivity and credentials before uploading",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=float(os.getenv("OPENOBSERVE_UPLOAD_INTERVAL", "2")),
        help="seconds between follow passes (default: 2)",
    )
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
    if args.batch_size <= 0 or args.retries < 0 or args.poll_interval <= 0:
        raise RuntimeError(
            "batch size and poll interval must be positive; retries must be non-negative"
        )
    if args.follow:
        args.incremental = True
        if len(args.paths) != 1:
            raise RuntimeError("follow mode accepts exactly one log file")
        if not args.run_id:
            raise RuntimeError("follow mode requires --run-id")
        if args.force:
            raise RuntimeError("--force cannot be combined with --follow")

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
    if args.preflight and not args.dry_run:
        try:
            preflight(url, org, stream, username, password)
            print(
                f"OpenObserve preflight ok: org={org} stream={stream}",
                file=sys.stderr,
            )
        except RuntimeError as exc:
            if not args.follow:
                raise
            print(
                f"OpenObserve preflight warning: {exc}; live uploader will retry",
                file=sys.stderr,
            )

    if not args.follow:
        summary = upload_once(
            args,
            url=url,
            org=org,
            stream=stream,
            endpoint=endpoint,
            username=username,
            password=password,
            state=state,
        )
        print(json.dumps(summary, sort_keys=True))
        if summary["valid"] == 0 and summary["skipped"] == 0:
            return 1
        return 0

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    totals = {"valid": 0, "invalid": 0, "uploaded": 0, "skipped": 0, "files": 1}
    final_error: RuntimeError | OSError | None = None
    while not STOP_REQUESTED:
        try:
            summary = upload_once(
                args,
                url=url,
                org=org,
                stream=stream,
                endpoint=endpoint,
                username=username,
                password=password,
                state=state,
            )
            merge_summary(totals, summary)
            if summary["uploaded"]:
                checkpoint = state.get(
                    incremental_state_key(url, org, stream, args.run_id), 0
                )
                print(
                    f"OpenObserve live upload: run_id={args.run_id} "
                    f"uploaded={summary['uploaded']} checkpoint={checkpoint}",
                    file=sys.stderr,
                )
            final_error = None
        except (OSError, RuntimeError) as exc:
            final_error = exc
            print(
                f"OpenObserve live upload warning: {exc}; local logs are intact",
                file=sys.stderr,
            )
        if not STOP_REQUESTED:
            time.sleep(args.poll_interval)

    # The producer and tee are stopped before the wrapper terminates us, so this
    # final pass closes the small race between the last poll and process exit.
    try:
        summary = upload_once(
            args,
            url=url,
            org=org,
            stream=stream,
            endpoint=endpoint,
            username=username,
            password=password,
            state=state,
        )
        merge_summary(totals, summary)
        final_error = None
    except (OSError, RuntimeError) as exc:
        final_error = exc
        print(
            f"OpenObserve final upload failed: {exc}; local logs are intact",
            file=sys.stderr,
        )
    print(json.dumps(totals, sort_keys=True))
    return 1 if final_error else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as exc:
        print(f"openobserve ingest error: {exc}", file=sys.stderr)
        raise SystemExit(1)
