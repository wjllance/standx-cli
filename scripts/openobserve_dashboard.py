#!/usr/bin/env python3
"""Create or update the native StandX maker dashboard in OpenObserve."""

from __future__ import annotations

import base64
import json
import os
import re
import sys
import time
from typing import Any
from urllib import error, parse, request


DASHBOARD_TITLE = "StandX Maker Overview"
NAME_RE = re.compile(r"^[A-Za-z0-9_]+$")


def axis(column: str, label: str | None = None) -> dict[str, Any]:
    return {"label": label or column, "alias": column, "column": column}


def empty_filter() -> dict[str, Any]:
    return {
        "type": "list",
        "values": [],
        "logicalOperator": "AND",
        "filterType": "list",
    }


def query(
    stream: str,
    sql: str,
    x: list[dict[str, Any]],
    y: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "query": sql,
        "customQuery": True,
        "fields": {
            "stream": stream,
            "stream_type": "logs",
            "x": x,
            "y": y,
            "z": [],
            "filter": empty_filter(),
        },
        "config": {"promql_legend": "", "layer_type": "scatter", "limit": 0},
    }


def panel(
    panel_id: str,
    chart_type: str,
    title: str,
    description: str,
    panel_query: dict[str, Any],
    layout: tuple[int, int, int, int, int],
    *,
    unit: str | None = None,
    decimals: int | None = 2,
    legends: bool = False,
) -> dict[str, Any]:
    x, y, w, h, index = layout
    config: dict[str, Any] = {
        "show_legends": legends,
        "legends_position": "bottom" if legends else None,
        "show_gridlines": chart_type in {"line", "area", "bar"},
        "connect_nulls": False,
    }
    if unit:
        config["unit"] = unit
    if decimals is not None:
        config["decimals"] = decimals
    return {
        "id": panel_id,
        "type": chart_type,
        "title": title,
        "description": description,
        "config": config,
        "queryType": "sql",
        "queries": [panel_query],
        "layout": {"x": x, "y": y, "w": w, "h": h, "i": index},
        "htmlContent": "",
        "markdownContent": "",
    }


def markdown_panel(content: str) -> dict[str, Any]:
    return {
        "id": "standx_pnl_note",
        "type": "markdown",
        "title": "Interpretation",
        "description": "",
        "config": {"show_legends": False},
        "queryType": "sql",
        "queries": [
            {
                "query": None,
                "customQuery": False,
                "fields": {
                    "stream": "",
                    "stream_type": "logs",
                    "x": [],
                    "y": [],
                    "z": [],
                    "filter": empty_filter(),
                },
                "config": {"promql_legend": ""},
            }
        ],
        "layout": {"x": 0, "y": 29, "w": 192, "h": 4, "i": 11},
        "htmlContent": "",
        "markdownContent": content,
    }


def build_dashboard(stream: str, latest_run: str) -> dict[str, Any]:
    selected = "run_id = '$run_id'"
    cycles = f"action = 'cycle_summary' AND {selected}"
    overview = [
        panel(
            "standx_fills",
            "metric",
            "Fills",
            "Fill events in the selected maker run and dashboard time range.",
            query(
                stream,
                f'SELECT count(*) AS fills FROM "{stream}" WHERE action = \'fill\' AND {selected}',
                [],
                [axis("fills", "Fills")],
            ),
            (0, 0, 48, 5, 1),
            decimals=0,
        ),
        panel(
            "standx_uptime",
            "metric",
            "Average Uptime",
            "Average quote uptime across cycle summaries.",
            query(
                stream,
                f'SELECT round(avg(uptime_pct), 2) AS avg_uptime_pct FROM "{stream}" WHERE {cycles}',
                [],
                [axis("avg_uptime_pct", "Uptime")],
            ),
            (48, 0, 48, 5, 2),
            unit="percent",
        ),
        panel(
            "standx_latest_pnl",
            "metric",
            "Latest Maker PnL",
            "Latest strategy-session PnL point. External inventory seed cost is not included.",
            query(
                stream,
                f'SELECT round(pnl, 4) AS latest_pnl FROM "{stream}" WHERE {cycles} ORDER BY _timestamp DESC LIMIT 1',
                [],
                [axis("latest_pnl", "PnL")],
            ),
            (96, 0, 48, 5, 3),
            unit="currency-dollar",
            decimals=4,
        ),
        panel(
            "standx_max_inventory",
            "metric",
            "Max |Inventory|",
            "Maximum absolute maker position observed in cycle summaries.",
            query(
                stream,
                f'SELECT round(max(abs(position)), 4) AS max_abs_position FROM "{stream}" WHERE {cycles}',
                [],
                [axis("max_abs_position", "Max inventory")],
            ),
            (144, 0, 48, 5, 4),
            decimals=4,
        ),
        panel(
            "standx_pnl_trend",
            "line",
            "Maker PnL",
            "Strategy-session PnL over time for the selected run.",
            query(
                stream,
                f'SELECT histogram(_timestamp) AS ts, avg(pnl) AS pnl FROM "{stream}" WHERE {cycles} GROUP BY histogram(_timestamp) ORDER BY ts',
                [axis("ts", "Time")],
                [axis("pnl", "PnL")],
            ),
            (0, 5, 96, 8, 5),
            unit="currency-dollar",
            decimals=4,
        ),
        panel(
            "standx_position_trend",
            "line",
            "Inventory",
            "Average maker position by time bucket.",
            query(
                stream,
                f'SELECT histogram(_timestamp) AS ts, avg(position) AS position FROM "{stream}" WHERE {cycles} GROUP BY histogram(_timestamp) ORDER BY ts',
                [axis("ts", "Time")],
                [axis("position", "Position")],
            ),
            (96, 5, 96, 8, 6),
            decimals=4,
        ),
        panel(
            "standx_uptime_trend",
            "line",
            "Quote Uptime",
            "Quote uptime by time bucket; drops expose disconnect/fail-safe windows.",
            query(
                stream,
                f'SELECT histogram(_timestamp) AS ts, avg(uptime_pct) AS uptime_pct FROM "{stream}" WHERE {cycles} GROUP BY histogram(_timestamp) ORDER BY ts',
                [axis("ts", "Time")],
                [axis("uptime_pct", "Uptime")],
            ),
            (0, 13, 96, 8, 7),
            unit="percent",
        ),
        panel(
            "standx_market_trend",
            "line",
            "Market / Spread",
            "Mark, best bid, and best ask observed by the maker.",
            query(
                stream,
                f'SELECT histogram(_timestamp) AS ts, avg(cast(mark AS DOUBLE)) AS mark, avg(cast(best_bid AS DOUBLE)) AS best_bid, avg(cast(best_ask AS DOUBLE)) AS best_ask FROM "{stream}" WHERE {cycles} GROUP BY histogram(_timestamp) ORDER BY ts',
                [axis("ts", "Time")],
                [axis("mark", "Mark"), axis("best_bid", "Best bid"), axis("best_ask", "Best ask")],
            ),
            (96, 13, 96, 8, 8),
            decimals=4,
            legends=True,
        ),
        panel(
            "standx_cancel_reasons",
            "bar",
            "Cancel Reasons",
            "Maker order cancellations grouped by reason.",
            query(
                stream,
                f'''SELECT coalesce(reason, 'unknown') AS reason, count(*) AS cancels
FROM "{stream}" WHERE action = 'cancel' AND {selected}
GROUP BY reason ORDER BY cancels DESC''',
                [axis("reason", "Reason")],
                [axis("cancels", "Cancels")],
            ),
            (0, 21, 64, 8, 9),
            decimals=0,
        ),
        panel(
            "standx_selected_run",
            "table",
            "Selected Run Snapshot",
            "Latest cycle summary for the selected run.",
            query(
                stream,
                f'''SELECT _timestamp, run_id, symbol, mode, fills_total, pnl, uptime_pct, position, config_hash
FROM "{stream}" WHERE {cycles}
ORDER BY _timestamp DESC LIMIT 1''',
                [axis("_timestamp", "Time"), axis("run_id", "Run"), axis("symbol", "Symbol"), axis("mode", "Mode"), axis("config_hash", "Config")],
                [axis("fills_total", "Fills"), axis("pnl", "PnL"), axis("uptime_pct", "Uptime"), axis("position", "Position")],
            ),
            (64, 21, 128, 8, 10),
            decimals=None,
        ),
        markdown_panel(
            "**PnL scope:** values are emitted by the maker session. The manually seeded `0.2 XAG` used for the reduce-only inventory-exit drill is external inventory, so its acquisition cost is not part of maker-session PnL. Use the Inventory and Events panels to evaluate that drill."
        ),
    ]

    runs_and_events = [
        panel(
            "standx_alerts",
            "metric",
            "Alerts / Breakers",
            "Alert events for the selected run.",
            query(
                stream,
                f'SELECT count(*) AS alerts FROM "{stream}" WHERE action = \'alert\' AND {selected}',
                [],
                [axis("alerts", "Alerts")],
            ),
            (0, 0, 64, 5, 1),
            decimals=0,
        ),
        panel(
            "standx_cleanups",
            "metric",
            "Maker Cleanups",
            "Structured maker order cleanup events for the selected run.",
            query(
                stream,
                f'SELECT count(*) AS cleanups FROM "{stream}" WHERE action = \'maker_cleanup\' AND {selected}',
                [],
                [axis("cleanups", "Cleanups")],
            ),
            (64, 0, 64, 5, 2),
            decimals=0,
        ),
        panel(
            "standx_inventory_exits",
            "metric",
            "Inventory Exits",
            "Reduce-only inventory-exit events for the selected run.",
            query(
                stream,
                f'SELECT count(*) AS exits FROM "{stream}" WHERE action = \'inventory_exit\' AND {selected}',
                [],
                [axis("exits", "Exits")],
            ),
            (128, 0, 64, 5, 3),
            decimals=0,
        ),
        panel(
            "standx_run_comparison",
            "table",
            "Run Comparison",
            "Latest cycle per run in the time range. This table intentionally ignores the run selector.",
            query(
                stream,
                f'''WITH ranked AS (
  SELECT run_id, symbol, mode, config_hash, _timestamp, fills_total, pnl, uptime_pct,
         row_number() OVER (PARTITION BY run_id ORDER BY _timestamp DESC) AS rn
  FROM "{stream}" WHERE action = 'cycle_summary'
)
SELECT _timestamp, run_id, symbol, mode, fills_total, pnl, uptime_pct, config_hash
FROM ranked WHERE rn = 1 ORDER BY _timestamp DESC LIMIT 50''',
                [axis("_timestamp", "Time"), axis("run_id", "Run"), axis("symbol", "Symbol"), axis("mode", "Mode"), axis("config_hash", "Config")],
                [axis("fills_total", "Fills"), axis("pnl", "PnL"), axis("uptime_pct", "Uptime")],
            ),
            (0, 5, 192, 10, 4),
            decimals=None,
        ),
        panel(
            "standx_key_events",
            "table",
            "Ledger, Fail-safe, Cleanup, Exit and Fill Events",
            "Operational and session-ledger event timeline for the selected run.",
            query(
                stream,
                f'''SELECT _timestamp, action, event, side, price, qty, reason, message
FROM "{stream}" WHERE {selected}
  AND action IN ('fill', 'cancel', 'alert', 'maker_cleanup', 'inventory_exit',
                 'order_response_reconnect', 'position_reconciliation',
                 'ledger_sync', 'inventory_adopted', 'startup_rejected', 'lifecycle')
ORDER BY _timestamp DESC LIMIT 200''',
                [axis("_timestamp", "Time"), axis("action", "Action"), axis("event", "Event"), axis("side", "Side"), axis("reason", "Reason"), axis("message", "Message")],
                [axis("price", "Price"), axis("qty", "Qty")],
            ),
            (0, 15, 192, 12, 5),
            decimals=None,
        ),
    ]

    return {
        "version": 8,
        "dashboardId": "",
        "title": DASHBOARD_TITLE,
        "description": "Maker paper/live validation: uptime, fills, maker-session PnL, inventory, cancellations, fail-safe cleanup, and inventory exit evidence.",
        "role": "",
        "owner": "",
        "tabs": [
            {"tabId": "default", "name": "Overview", "panels": overview},
            {"tabId": "runs-events", "name": "Runs & Events", "panels": runs_and_events},
        ],
        "variables": {
            "list": [
                {
                    "type": "query_values",
                    "name": "run_id",
                    "label": "Maker run",
                    "query_data": {
                        "stream_type": "logs",
                        "stream": stream,
                        "field": "run_id",
                        "max_record_size": 100,
                    },
                    # Empty lets query-values select the newest returned run after the
                    # options load, avoiding an initial double refresh in OpenObserve.
                    "value": "",
                    "options": [],
                    "multiSelect": False,
                    "hideOnDashboard": False,
                    "escapeSingleQuotes": True,
                }
            ],
            "showDynamicFilters": False,
        },
        "defaultDatetimeDuration": {
            "type": "relative",
            "relativeTimePeriod": "24h",
            "startTime": None,
            "endTime": None,
        },
    }


class OpenObserve:
    def __init__(self) -> None:
        self.url = os.getenv("OPENOBSERVE_URL", "http://127.0.0.1:5080").rstrip("/")
        self.org = os.getenv("OPENOBSERVE_ORG", "default")
        self.stream = os.getenv("OPENOBSERVE_STREAM", "standx_maker")
        username = os.getenv("OPENOBSERVE_USER", "")
        password = os.getenv("OPENOBSERVE_PASSWORD", "")
        if not username or not password:
            raise RuntimeError("OPENOBSERVE_USER and OPENOBSERVE_PASSWORD are required")
        if not NAME_RE.fullmatch(self.org) or not NAME_RE.fullmatch(self.stream):
            raise RuntimeError("OpenObserve org and stream may contain only letters, digits, and underscore")
        credential = base64.b64encode(f"{username}:{password}".encode()).decode()
        self.headers = {
            "Authorization": f"Basic {credential}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            "User-Agent": "standx-openobserve-dashboard/1",
        }

    def json_request(
        self, method: str, path: str, payload: dict[str, Any] | None = None
    ) -> dict[str, Any]:
        data = None if payload is None else json.dumps(payload).encode()
        req = request.Request(self.url + path, data=data, headers=self.headers, method=method)
        try:
            with request.urlopen(req, timeout=30) as response:
                body = response.read()
                return json.loads(body) if body else {}
        except error.HTTPError as exc:
            detail = exc.read(4096).decode(errors="replace")
            raise RuntimeError(f"OpenObserve {method} {path} returned HTTP {exc.code}: {detail}") from exc
        except (error.URLError, TimeoutError) as exc:
            raise RuntimeError(f"OpenObserve {method} {path} failed: {exc}") from exc

    def latest_run(self) -> str:
        now_us = int(time.time() * 1_000_000)
        sql = (
            f'SELECT run_id FROM "{self.stream}" '
            "WHERE action = 'cycle_summary' ORDER BY _timestamp DESC LIMIT 1"
        )
        result = self.json_request(
            "POST",
            f"/api/{parse.quote(self.org, safe='')}/_search?type=logs",
            {
                "query": {
                    "sql": sql,
                    "start_time": now_us - 365 * 24 * 3600 * 1_000_000,
                    "end_time": now_us,
                    "from": 0,
                    "size": 1,
                }
            },
        )
        hits = result.get("hits", [])
        if not hits or not hits[0].get("run_id"):
            raise RuntimeError(f"no cycle_summary events found in stream {self.stream}")
        return str(hits[0]["run_id"])

    def upsert_dashboard(self, dashboard: dict[str, Any]) -> tuple[str, str, str]:
        org = parse.quote(self.org, safe="")
        title = parse.quote(DASHBOARD_TITLE, safe="")
        listing = self.json_request("GET", f"/api/{org}/dashboards?title={title}")
        matches = [item for item in listing.get("dashboards", []) if item.get("title") == DASHBOARD_TITLE]
        if matches:
            current = matches[0]
            dashboard_id = str(current["dashboard_id"])
            folder = str(current.get("folder_id") or "default")
            dashboard["dashboardId"] = dashboard_id
            params = parse.urlencode({"folder": folder, "hash": current.get("hash", "")})
            saved = self.json_request(
                "PUT",
                f"/api/{org}/dashboards/{parse.quote(dashboard_id, safe='')}?{params}",
                dashboard,
            )
            saved_dashboard = saved.get("v8") or saved
            return "updated", str(saved_dashboard.get("dashboardId") or dashboard_id), folder

        saved = self.json_request("POST", f"/api/{org}/dashboards?folder=default", dashboard)
        saved_dashboard = saved.get("v8") or saved
        dashboard_id = str(saved_dashboard.get("dashboardId") or "")
        if not dashboard_id:
            raise RuntimeError("dashboard create response did not include dashboardId")
        return "created", dashboard_id, "default"


def main() -> int:
    client = OpenObserve()
    latest_run = client.latest_run()
    dashboard = build_dashboard(client.stream, latest_run)
    action, dashboard_id, folder = client.upsert_dashboard(dashboard)
    params = parse.urlencode(
        {
            "org_identifier": client.org,
            "dashboard": dashboard_id,
            "folder": folder,
            "tab": "default",
        }
    )
    print(
        json.dumps(
            {
                "action": action,
                "dashboard_id": dashboard_id,
                "default_run": latest_run,
                "folder": folder,
                "panels": sum(len(tab["panels"]) for tab in dashboard["tabs"]),
                "url": f"{client.url}/web/dashboards/view?{params}",
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"openobserve dashboard error: {exc}", file=sys.stderr)
        raise SystemExit(1)
