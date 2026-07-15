#!/usr/bin/env python3
"""Offline contract tests for the phase-1 OpenObserve dashboard."""

from __future__ import annotations

import unittest

import openobserve_dashboard as dashboard


class DashboardContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.payload = dashboard.build_dashboard("standx_maker", "latest-run")
        self.tabs = {tab["tabId"]: tab for tab in self.payload["tabs"]}
        self.phase_one = {
            panel["id"]: panel
            for panel in self.tabs["performance-latency"]["panels"]
        }
        self.overview = {
            panel["id"]: panel for panel in self.tabs["default"]["panels"]
        }
        self.runs_and_events = {
            panel["id"]: panel for panel in self.tabs["runs-events"]["panels"]
        }

    def query_sql(self, panel_id: str) -> str:
        return self.phase_one[panel_id]["queries"][0]["query"]

    def run_event_query_sql(self, panel_id: str) -> str:
        return self.runs_and_events[panel_id]["queries"][0]["query"]

    def test_phase_one_tab_has_unique_required_panels(self) -> None:
        self.assertEqual(self.payload["version"], 8)
        self.assertEqual(
            set(self.phase_one),
            {
                "standx_net_pnl_attribution",
                "standx_markout",
                "standx_time_weighted_quotes",
                "standx_order_latency_summary",
                "standx_order_latency_events",
                "standx_account_event_lag",
                "standx_performance_run_comparison",
                "standx_latency_run_comparison",
            },
        )

    def test_selected_run_queries_keep_run_selector_and_phase_one_fields(self) -> None:
        attribution = self.query_sql("standx_net_pnl_attribution")
        for field in (
            "passive_cashflow_quote",
            "passive_capture_bps",
            "exit_cashflow_quote",
            "gross_spread_quote",
            "net_pnl_quote",
        ):
            self.assertIn(field, attribution)
        quote_time = self.query_sql("standx_time_weighted_quotes")
        for field in (
            "time_weighted_uptime_pct",
            "eligible_bid_qty_ms",
            "inventory_nonzero_ms",
            "inventory_abs_qty_ms",
        ):
            self.assertIn(field, quote_time)
        latency = self.query_sql("standx_order_latency_summary")
        for field in (
            "write_p95_ms",
            "ack_p95_ms",
            "effective_latency_p95_ms",
        ):
            self.assertIn(field, latency)
        self.assertNotIn("fill_after_cancel", latency)
        self.assertNotIn(
            "fill_after_cancel",
            self.query_sql("standx_order_latency_events"),
        )
        self.assertNotIn(
            "timeout_phase",
            self.query_sql("standx_order_latency_events"),
        )
        self.assertNotIn(
            "timeout_ms",
            self.query_sql("standx_order_latency_events"),
        )
        for panel_id in (
            "standx_net_pnl_attribution",
            "standx_markout",
            "standx_time_weighted_quotes",
            "standx_order_latency_summary",
            "standx_order_latency_events",
            "standx_account_event_lag",
        ):
            self.assertIn("run_id = '$run_id'", self.query_sql(panel_id))

    def test_comparison_queries_group_by_run_and_surface_config_hash(self) -> None:
        for panel_id in (
            "standx_performance_run_comparison",
            "standx_latency_run_comparison",
        ):
            sql = self.query_sql(panel_id)
            self.assertIn("run_id", sql)
            self.assertIn("config_hash", sql)
            self.assertNotIn("$run_id", sql)

    def test_runs_events_queries_surface_real_exit_events_and_stop_context(self) -> None:
        exits = self.run_event_query_sql("standx_inventory_exits")
        self.assertIn("action = 'inventory_exit_submitted'", exits)
        self.assertIn("run_id = '$run_id'", exits)

        timeline = self.run_event_query_sql("standx_key_events")
        self.assertIn("inventory_exit_submitted", timeline)

        comparison = self.run_event_query_sql("standx_run_comparison")
        for field in (
            "position",
            "starting_position",
            "lifecycle_event",
            "lifecycle_message",
        ):
            self.assertIn(field, comparison)
        self.assertNotIn("$run_id", comparison)

    def test_dashboard_uses_roomy_operational_layout_and_recent_default_window(self) -> None:
        self.assertEqual(
            self.payload["defaultDatetimeDuration"]["relativeTimePeriod"], "6h"
        )
        for panel_id in (
            "standx_fills",
            "standx_uptime",
            "standx_latest_pnl",
            "standx_max_inventory",
        ):
            self.assertEqual(self.overview[panel_id]["layout"]["h"], 8)

        self.assertEqual(self.overview["standx_pnl_trend"]["layout"]["h"], 10)
        self.assertEqual(self.overview["standx_position_trend"]["layout"]["h"], 10)
        self.assertEqual(
            self.runs_and_events["standx_run_comparison"]["layout"]["h"], 13
        )
        self.assertEqual(
            self.runs_and_events["standx_key_events"]["layout"]["h"], 16
        )

    def test_every_panel_query_targets_the_configured_stream(self) -> None:
        for tab in self.payload["tabs"]:
            for panel in tab["panels"]:
                sql = panel["queries"][0]["query"]
                if sql is not None:
                    self.assertIn('"standx_maker"', sql)


if __name__ == "__main__":
    unittest.main()
