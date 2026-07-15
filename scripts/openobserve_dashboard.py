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
        "layout": {"x": 0, "y": 37, "w": 192, "h": 4, "i": 11},
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
                f'SELECT count(DISTINCT event_id) AS fills FROM "{stream}" WHERE action = \'fill\' AND {selected}',
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
            "standx_account_trend",
            "line",
            "Equity / uPnL / Available",
            "Authenticated account equity, unrealized PnL, and cross-available margin (live runs only; account is null in paper mode).",
            query(
                stream,
                f'''SELECT histogram(_timestamp) AS ts,
       avg(cast(account_equity AS DOUBLE)) AS equity,
       avg(cast(account_upnl AS DOUBLE)) AS upnl,
       avg(cast(account_available AS DOUBLE)) AS available
FROM "{stream}" WHERE {cycles}
GROUP BY histogram(_timestamp) ORDER BY ts''',
                [axis("ts", "Time")],
                [axis("equity", "Equity"), axis("upnl", "uPnL"), axis("available", "Available")],
            ),
            (0, 21, 96, 8, 12),
            unit="currency-dollar",
            legends=True,
        ),
        panel(
            "standx_data_freshness",
            "table",
            "Data Freshness",
            "Newest ingested event for the selected run. A stale max(_timestamp) signals a stalled maker or a stalled upload pipeline.",
            query(
                stream,
                f'''SELECT max(_timestamp) AS last_event,
       count(DISTINCT event_id) AS events
FROM "{stream}" WHERE {selected}''',
                [axis("last_event", "Last event")],
                [axis("events", "Events")],
            ),
            (96, 21, 96, 8, 13),
            decimals=None,
        ),
        panel(
            "standx_cancel_reasons",
            "bar",
            "Cancel Reasons",
            "Maker order cancellations grouped by reason.",
            query(
                stream,
                f'''SELECT coalesce(reason, 'unknown') AS reason, count(DISTINCT event_id) AS cancels
FROM "{stream}" WHERE action = 'cancel' AND {selected}
GROUP BY reason ORDER BY cancels DESC''',
                [axis("reason", "Reason")],
                [axis("cancels", "Cancels")],
            ),
            (0, 29, 64, 8, 9),
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
            (64, 29, 128, 8, 10),
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
                f'SELECT count(DISTINCT event_id) AS alerts FROM "{stream}" WHERE action = \'alert\' AND {selected}',
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
                f"SELECT count(DISTINCT event_id) AS cleanups FROM \"{stream}\" WHERE action = 'maker_cleanup' AND event = 'complete' AND {selected}",
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
                f'SELECT count(DISTINCT event_id) AS exits FROM "{stream}" WHERE action = \'inventory_exit_submitted\' AND {selected}',
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
            "Latest cycle per run plus its latest lifecycle detail, including fail-safe stop reasons. This table intentionally ignores the run selector.",
            query(
                stream,
                f'''WITH latest_cycles AS (
  SELECT run_id, symbol, mode, config_hash, _timestamp, fills_total, pnl, uptime_pct,
         position, starting_position,
         row_number() OVER (PARTITION BY run_id ORDER BY _timestamp DESC) AS rn
  FROM "{stream}" WHERE action = 'cycle_summary'
), latest_lifecycle AS (
  SELECT run_id, event AS lifecycle_event, message AS lifecycle_message,
         row_number() OVER (PARTITION BY run_id ORDER BY _timestamp DESC) AS rn
  FROM "{stream}" WHERE action = 'lifecycle'
)
SELECT cycles._timestamp, cycles.run_id, cycles.symbol, cycles.mode, cycles.fills_total,
       cycles.pnl, cycles.uptime_pct, cycles.position, cycles.starting_position,
       cycles.config_hash, lifecycle.lifecycle_event, lifecycle.lifecycle_message
FROM latest_cycles AS cycles
LEFT JOIN latest_lifecycle AS lifecycle
  ON cycles.run_id = lifecycle.run_id AND lifecycle.rn = 1
WHERE cycles.rn = 1 ORDER BY cycles._timestamp DESC LIMIT 50''',
                [axis("_timestamp", "Time"), axis("run_id", "Run"), axis("symbol", "Symbol"), axis("mode", "Mode"), axis("lifecycle_event", "Lifecycle"), axis("lifecycle_message", "Lifecycle detail"), axis("config_hash", "Config")],
                [axis("fills_total", "Fills"), axis("pnl", "PnL"), axis("uptime_pct", "Uptime"), axis("position", "Position"), axis("starting_position", "Starting position")],
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
                f'''SELECT _timestamp, action, kind, severity, event, side, price, qty,
       request_id, request_kind, timeout_phase, recovery_target, age_ms, timeout_ms,
       position_delta, expected_position, observed_position, reason, message
FROM "{stream}" WHERE {selected}
  AND action IN ('fill', 'cancel', 'alert', 'risk_notification', 'maker_cleanup', 'inventory_exit_submitted',
                 'order_response_reconnect', 'position_reconciliation',
                 'account_trade_shadow', 'ledger_sync', 'inventory_adopted',
                 'startup_rejected', 'lifecycle', 'performance_summary',
                 'order_latency', 'order_latency_summary')
ORDER BY _timestamp DESC LIMIT 200''',
                [axis("_timestamp", "Time"), axis("action", "Action"), axis("kind", "Kind"), axis("severity", "Severity"), axis("event", "Event"), axis("request_id", "Request"), axis("request_kind", "Request Kind"), axis("timeout_phase", "Timeout Phase"), axis("recovery_target", "Recovery Target"), axis("side", "Side"), axis("reason", "Reason"), axis("message", "Message")],
                [axis("price", "Price"), axis("qty", "Qty"), axis("age_ms", "Age"), axis("timeout_ms", "Timeout"), axis("position_delta", "Position Delta"), axis("expected_position", "Expected Position"), axis("observed_position", "Observed Position")],
            ),
            (0, 15, 192, 12, 5),
            decimals=None,
        ),
        panel(
            "standx_error_reject",
            "bar",
            "Rejections & Error Signals",
            "Order rejections, startup rejections, warning/critical risk notifications, and reconciliation/cleanup precursor failures for the selected run.",
            query(
                stream,
                f'''SELECT action AS action, count(DISTINCT event_id) AS events
FROM "{stream}" WHERE {selected}
  AND (action IN ('place_rejected', 'place_rejected_async', 'startup_rejected')
       OR (action = 'risk_notification' AND severity IN ('warning', 'critical'))
       OR (action = 'position_reconciliation' AND event IN ('frozen', 'snapshot_failed'))
       OR (action = 'maker_cleanup' AND event = 'retry_incomplete'))
GROUP BY action ORDER BY events DESC''',
                [axis("action", "Signal")],
                [axis("events", "Events")],
            ),
            (0, 27, 96, 8, 6),
            decimals=0,
        ),
        panel(
            "standx_stream_health",
            "bar",
            "Stream Health / Reconnects",
            "Account/order-response reconnect lifecycle events by phase; repeated attempts or failures expose disconnect windows.",
            query(
                stream,
                f'''SELECT coalesce(event, 'unknown') AS event, count(DISTINCT event_id) AS events
FROM "{stream}" WHERE action = 'order_response_reconnect' AND {selected}
GROUP BY event ORDER BY events DESC''',
                [axis("event", "Phase")],
                [axis("events", "Events")],
            ),
            (96, 27, 96, 8, 7),
            decimals=0,
        ),
    ]

    performance_latency = [
        panel(
            "standx_net_pnl_attribution",
            "table",
            "Net PnL Attribution",
            "Latest phase-1 performance summary. Missing convertible execution costs remain explicitly counted.",
            query(
                stream,
                f'''SELECT _timestamp, passive_fills, passive_qty, exit_fills, exit_qty,
       passive_cashflow_quote, passive_capture_bps, exit_cashflow_quote,
       gross_spread_quote, inventory_mtm_change_quote, rebate_quote, fee_quote,
       funding_quote, funding_available, exit_cost_quote, net_pnl_quote,
       net_pnl_complete, execution_costs_unavailable
FROM "{stream}" WHERE action = 'performance_summary' AND {selected}
ORDER BY _timestamp DESC LIMIT 1''',
                [axis("_timestamp", "Time")],
                [
                    axis("gross_spread_quote", "Gross spread"),
                    axis("inventory_mtm_change_quote", "Inventory MTM"),
                    axis("rebate_quote", "Rebate"),
                    axis("fee_quote", "Fee"),
                    axis("funding_quote", "Funding"),
                    axis("exit_cost_quote", "Exit cost"),
                    axis("net_pnl_quote", "Net PnL"),
                ],
            ),
            (0, 0, 192, 8, 1),
            decimals=None,
        ),
        panel(
            "standx_markout",
            "table",
            "Post-fill Markout",
            "Quantity-weighted 1s/5s/30s markout. Unavailable windows are never filled with the current mark.",
            query(
                stream,
                f'''SELECT _timestamp, markout_1s_bps, markout_5s_bps, markout_30s_bps,
       markout_1s_unavailable, markout_5s_unavailable, markout_30s_unavailable
FROM "{stream}" WHERE action = 'performance_summary' AND {selected}
ORDER BY _timestamp DESC LIMIT 1''',
                [axis("_timestamp", "Time")],
                [
                    axis("markout_1s_bps", "1s bps"),
                    axis("markout_5s_bps", "5s bps"),
                    axis("markout_30s_bps", "30s bps"),
                ],
            ),
            (0, 8, 96, 8, 2),
            decimals=None,
        ),
        panel(
            "standx_time_weighted_quotes",
            "table",
            "Time-weighted Quote Quality",
            "Two-sided uptime and eligible bid/ask depth-time integrals from monotonic quote intervals.",
            query(
                stream,
                f'''SELECT _timestamp, time_weighted_uptime_pct, eligible_bid_qty_ms,
       eligible_ask_qty_ms, eligible_total_qty_ms, inventory_observed_ms,
       inventory_nonzero_ms, inventory_abs_qty_ms, inventory_avg_abs_qty
FROM "{stream}" WHERE action = 'performance_summary' AND {selected}
ORDER BY _timestamp DESC LIMIT 1''',
                [axis("_timestamp", "Time")],
                [
                    axis("time_weighted_uptime_pct", "Uptime"),
                    axis("eligible_bid_qty_ms", "Bid qty-ms"),
                    axis("eligible_ask_qty_ms", "Ask qty-ms"),
                    axis("eligible_total_qty_ms", "Total qty-ms"),
                    axis("inventory_nonzero_ms", "Inventory nonzero ms"),
                    axis("inventory_abs_qty_ms", "Abs inventory qty-ms"),
                ],
            ),
            (96, 8, 96, 8, 3),
            decimals=None,
        ),
        panel(
            "standx_order_latency_summary",
            "table",
            "Place / Cancel Latency Summary",
            "Write, ack and effective p50/p95/p99 with reject and censored timeout rates. Optional fill-after-cancel fields are omitted until the stream schema has observed them.",
            query(
                stream,
                f'''SELECT _timestamp, kind, requests, accepted, rejected, effective, timeout,
       invalidated, process_ended, pending, reject_rate, timeout_rate,
       write_p50_ms, write_p95_ms, write_p99_ms,
       ack_p50_ms, ack_p95_ms, ack_p99_ms,
       effective_latency_p50_ms, effective_latency_p95_ms, effective_latency_p99_ms
FROM "{stream}" WHERE action = 'order_latency_summary' AND {selected}
ORDER BY kind, _timestamp DESC LIMIT 10''',
                [axis("kind", "Kind"), axis("_timestamp", "Time")],
                [
                    axis("requests", "Requests"),
                    axis("reject_rate", "Reject rate"),
                    axis("timeout_rate", "Timeout rate"),
                    axis("ack_p95_ms", "Ack p95"),
                    axis("effective_latency_p95_ms", "Effective p95"),
                ],
            ),
            (0, 16, 192, 9, 4),
            decimals=None,
        ),
        panel(
            "standx_order_latency_events",
            "table",
            "Order Lifecycle Correlation",
            "Request-level lifecycle including account-order-before-ack, timeout, and invalidation.",
            query(
                stream,
                f'''SELECT _timestamp, request_id, kind, generation, cycle, symbol, side, level,
       market_source, recovery, outcome, timeout_phase, timeout_ms,
       place_write_ms, place_ack_ms, place_effective_ms,
       cancel_write_ms, cancel_ack_ms, cancel_effective_ms
FROM "{stream}" WHERE action = 'order_latency' AND {selected}
ORDER BY _timestamp DESC LIMIT 200''',
                [
                    axis("_timestamp", "Time"),
                    axis("request_id", "Request"),
                    axis("kind", "Kind"),
                    axis("outcome", "Outcome"),
                    axis("timeout_phase", "Timeout Phase"),
                ],
                [
                    axis("timeout_ms", "Timeout"),
                    axis("place_ack_ms", "Place ack"),
                    axis("place_effective_ms", "Place effective"),
                    axis("cancel_ack_ms", "Cancel ack"),
                    axis("cancel_effective_ms", "Cancel effective"),
                ],
            ),
            (0, 25, 192, 12, 5),
            decimals=None,
        ),
        panel(
            "standx_account_event_lag",
            "table",
            "Account Stream Event Lag",
            "Exchange event timestamp to local receipt lag by authenticated account channel.",
            query(
                stream,
                f'''SELECT channel,
       approx_percentile_cont(account_event_lag_ms, 0.50) AS p50_ms,
       approx_percentile_cont(account_event_lag_ms, 0.95) AS p95_ms,
       approx_percentile_cont(account_event_lag_ms, 0.99) AS p99_ms,
       count(*) AS samples
FROM "{stream}" WHERE action = 'account_event_lag' AND available = true AND {selected}
GROUP BY channel ORDER BY channel''',
                [axis("channel", "Channel")],
                [
                    axis("p50_ms", "p50"),
                    axis("p95_ms", "p95"),
                    axis("p99_ms", "p99"),
                    axis("samples", "Samples"),
                ],
            ),
            (0, 37, 192, 8, 6),
            decimals=None,
        ),
        panel(
            "standx_performance_run_comparison",
            "table",
            "Phase-1 Performance by Run / Config",
            "Latest deterministic performance summary per run for cross-config comparison. This table intentionally ignores the run selector.",
            query(
                stream,
                f'''WITH ranked AS (
  SELECT _timestamp, run_id, config_hash, symbol, passive_fills, exit_fills,
         passive_capture_bps, net_pnl_quote, markout_1s_bps, markout_5s_bps,
         markout_30s_bps, time_weighted_uptime_pct, inventory_nonzero_ms,
         inventory_abs_qty_ms, funding_available, net_pnl_complete,
         row_number() OVER (PARTITION BY run_id ORDER BY _timestamp DESC) AS rn
  FROM "{stream}" WHERE action = 'performance_summary'
)
SELECT _timestamp, run_id, config_hash, symbol, passive_fills, exit_fills,
       passive_capture_bps, net_pnl_quote, markout_1s_bps, markout_5s_bps,
       markout_30s_bps, time_weighted_uptime_pct, inventory_nonzero_ms,
       inventory_abs_qty_ms, funding_available, net_pnl_complete
FROM ranked WHERE rn = 1 ORDER BY _timestamp DESC LIMIT 50''',
                [
                    axis("_timestamp", "Time"),
                    axis("run_id", "Run"),
                    axis("config_hash", "Config"),
                    axis("symbol", "Symbol"),
                ],
                [
                    axis("net_pnl_quote", "Net PnL"),
                    axis("markout_5s_bps", "5s markout"),
                    axis("time_weighted_uptime_pct", "Uptime"),
                ],
            ),
            (0, 45, 192, 10, 7),
            decimals=None,
        ),
        panel(
            "standx_latency_run_comparison",
            "table",
            "Phase-1 Latency by Run / Config",
            "Latest place/cancel distribution per run and config, retaining timeout and reject rates. This table intentionally ignores the run selector.",
            query(
                stream,
                f'''WITH ranked AS (
  SELECT _timestamp, run_id, config_hash, symbol, kind, requests, reject_rate,
         timeout_rate, write_p95_ms, ack_p95_ms, effective_latency_p95_ms,
         row_number() OVER (PARTITION BY run_id, kind ORDER BY _timestamp DESC) AS rn
  FROM "{stream}" WHERE action = 'order_latency_summary'
)
SELECT _timestamp, run_id, config_hash, symbol, kind, requests, reject_rate,
       timeout_rate, write_p95_ms, ack_p95_ms, effective_latency_p95_ms
FROM ranked WHERE rn = 1 ORDER BY _timestamp DESC, kind LIMIT 100''',
                [
                    axis("_timestamp", "Time"),
                    axis("run_id", "Run"),
                    axis("config_hash", "Config"),
                    axis("symbol", "Symbol"),
                    axis("kind", "Kind"),
                ],
                [
                    axis("timeout_rate", "Timeout rate"),
                    axis("ack_p95_ms", "Ack p95"),
                    axis("effective_latency_p95_ms", "Effective p95"),
                ],
            ),
            (0, 55, 192, 10, 8),
            decimals=None,
        ),
    ]

    return {
        "version": 10,
        "dashboardId": "",
        "title": DASHBOARD_TITLE,
        "description": "Maker paper/live validation: uptime, fills, maker-session PnL, authenticated account-stream health, position jumps, reconciliation, cleanup, volatility breaker, and inventory exit evidence.",
        "role": "",
        "owner": "",
        "tabs": [
            {"tabId": "default", "name": "Overview", "panels": overview},
            {"tabId": "runs-events", "name": "Runs & Events", "panels": runs_and_events},
            {
                "tabId": "performance-latency",
                "name": "Performance & Latency",
                "panels": performance_latency,
            },
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
