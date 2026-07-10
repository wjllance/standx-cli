#!/usr/bin/env python3
"""Run read-only SQL against the configured OpenObserve log stream."""

from __future__ import annotations

import argparse
import base64
import json
import os
import sys
import time
from urllib import error, parse, request


DEFAULT_SQL = """
SELECT
  run_id,
  symbol,
  count(DISTINCT event_id) AS events,
  max(fills_total) AS fills,
  avg(uptime_pct) AS avg_uptime
FROM \"{stream}\"
WHERE action = 'cycle_summary'
GROUP BY run_id, symbol
ORDER BY events DESC
""".strip()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Query StandX events in OpenObserve")
    parser.add_argument("--sql", help="SQL query; defaults to per-run maker summary")
    parser.add_argument("--hours", type=float, default=24.0)
    parser.add_argument("--size", type=int, default=100)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.hours <= 0 or args.size <= 0:
        raise RuntimeError("hours and size must be positive")

    url = os.getenv("OPENOBSERVE_URL", "http://127.0.0.1:5080").rstrip("/")
    org = os.getenv("OPENOBSERVE_ORG", "default")
    stream = os.getenv("OPENOBSERVE_STREAM", "standx_maker")
    username = os.getenv("OPENOBSERVE_USER", "")
    password = os.getenv("OPENOBSERVE_PASSWORD", "")
    if not username or not password:
        raise RuntimeError("OPENOBSERVE_USER and OPENOBSERVE_PASSWORD are required")

    now_us = int(time.time() * 1_000_000)
    start_us = now_us - int(args.hours * 3600 * 1_000_000)
    sql = args.sql or DEFAULT_SQL.format(stream=stream)
    body = json.dumps(
        {
            "query": {
                "sql": sql,
                "start_time": start_us,
                "end_time": now_us,
                "from": 0,
                "size": args.size,
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
            "User-Agent": "standx-openobserve-query/1",
        },
        method="POST",
    )
    try:
        with request.urlopen(req, timeout=30) as response:
            result = json.load(response)
    except error.HTTPError as exc:
        detail = exc.read(2048).decode(errors="replace")
        raise RuntimeError(f"OpenObserve query returned HTTP {exc.code}: {detail}") from exc
    except (error.URLError, TimeoutError) as exc:
        raise RuntimeError(f"OpenObserve query failed: {exc}") from exc

    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"openobserve query error: {exc}", file=sys.stderr)
        raise SystemExit(1)
