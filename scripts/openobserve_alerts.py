#!/usr/bin/env python3
"""Create or update the StandX maker deadman alert in OpenObserve.

A scheduled alert fires when the ``standx_maker`` stream has seen no
``action='cycle_summary'`` event for the deadman window (~3 minutes). This is
the push-based safety net for issue #220: a silent death (SIGKILL / OOM /
panic / host down) stops emitting cycle summaries, so the deadman trips and
posts to the configured webhook even though the process itself can no longer
notify anyone.

Environment (shares the dashboard script's OpenObserve variables):

- ``OPENOBSERVE_URL``           default ``http://127.0.0.1:5080``
- ``OPENOBSERVE_ORG``           default ``default``
- ``OPENOBSERVE_STREAM``        default ``standx_maker``
- ``OPENOBSERVE_USER`` / ``OPENOBSERVE_PASSWORD``   required (Basic auth)
- ``OPENOBSERVE_ALERT_WEBHOOK`` required; Feishu (Lark) custom-bot webhook the
  alert POSTs to. The template body is Feishu msg_type=text, so a Slack or
  generic ``{"text": ...}`` endpoint will reject it.
- ``OPENOBSERVE_ALERT_MINUTES`` default ``3``; deadman window in minutes
"""

from __future__ import annotations

import base64
import json
import os
import re
import sys
from typing import Any
from urllib import error, parse, request


ALERT_NAME = "standx_maker_deadman"
TEMPLATE_NAME = "standx_maker_deadman_template"
DESTINATION_NAME = "standx_maker_deadman_webhook"
NAME_RE = re.compile(r"^[A-Za-z0-9_]+$")

# Feishu (Lark) custom-bot text payload. OpenObserve substitutes the {var}
# placeholders at send time; the JSON structure braces are left untouched
# because only recognized variable names are replaced. Feishu requires the
# msg_type/content envelope rather than a bare {"text": ...} body.
_DEADMAN_TEXT = (
    "\U0001f6d1 StandX maker DEADMAN: no cycle_summary in the "
    "{stream_name} stream for the deadman window. The maker may have "
    "died silently (SIGKILL/OOM/panic/host down) and could be leaving "
    "resting orders on the venue. Alert: {alert_name} org: {org_name}"
)
TEMPLATE_BODY = json.dumps(
    {"msg_type": "text", "content": {"text": _DEADMAN_TEXT}}
)


def build_alert(stream: str, minutes: int) -> dict[str, Any]:
    """Scheduled alert that trips when fewer than one cycle_summary row is
    seen within the deadman window."""
    return {
        "name": ALERT_NAME,
        "stream_type": "logs",
        "stream_name": stream,
        "is_real_time": False,
        "query_condition": {
            "type": "custom",
            "conditions": [
                {
                    "column": "action",
                    "operator": "=",
                    "value": "cycle_summary",
                }
            ],
            "sql": "",
            "promql": "",
            "promql_condition": None,
            "aggregation": None,
            "vrl_function": None,
            "search_event_type": None,
        },
        "trigger_condition": {
            # Count matching rows over the last `minutes`; fire when there are
            # none (< 1). Re-evaluate every minute and silence repeats for the
            # window so a prolonged outage does not spam the channel.
            "period": minutes,
            "operator": "<",
            "threshold": 1,
            "frequency": 1,
            "frequency_type": "minutes",
            "silence": minutes,
            "timezone": "UTC",
        },
        "destinations": [DESTINATION_NAME],
        "context_attributes": {},
        "row_template": "",
        "description": (
            "Deadman: fires when the maker stops emitting cycle_summary "
            "events, i.e. it likely died without running cleanup (issue #220)."
        ),
        "enabled": True,
    }


class OpenObserve:
    def __init__(self) -> None:
        self.url = os.getenv("OPENOBSERVE_URL", "http://127.0.0.1:5080").rstrip("/")
        self.org = os.getenv("OPENOBSERVE_ORG", "default")
        self.stream = os.getenv("OPENOBSERVE_STREAM", "standx_maker")
        self.webhook = os.getenv("OPENOBSERVE_ALERT_WEBHOOK", "")
        try:
            self.minutes = int(os.getenv("OPENOBSERVE_ALERT_MINUTES", "3"))
        except ValueError as exc:
            raise RuntimeError("OPENOBSERVE_ALERT_MINUTES must be an integer") from exc
        username = os.getenv("OPENOBSERVE_USER", "")
        password = os.getenv("OPENOBSERVE_PASSWORD", "")
        if not username or not password:
            raise RuntimeError("OPENOBSERVE_USER and OPENOBSERVE_PASSWORD are required")
        if not self.webhook:
            raise RuntimeError("OPENOBSERVE_ALERT_WEBHOOK is required")
        if self.minutes < 1:
            raise RuntimeError("OPENOBSERVE_ALERT_MINUTES must be >= 1")
        if not NAME_RE.fullmatch(self.org) or not NAME_RE.fullmatch(self.stream):
            raise RuntimeError(
                "OpenObserve org and stream may contain only letters, digits, and underscore"
            )
        credential = base64.b64encode(f"{username}:{password}".encode()).decode()
        self.headers = {
            "Authorization": f"Basic {credential}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            "User-Agent": "standx-openobserve-alerts/1",
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
            raise RuntimeError(
                f"OpenObserve {method} {path} returned HTTP {exc.code}: {detail}"
            ) from exc
        except (error.URLError, TimeoutError) as exc:
            raise RuntimeError(f"OpenObserve {method} {path} failed: {exc}") from exc

    def _org(self) -> str:
        return parse.quote(self.org, safe="")

    def _exists(self, path: str, name: str, key: str = "list") -> bool:
        listing = self.json_request("GET", path)
        items = listing.get(key, listing) if isinstance(listing, dict) else listing
        if not isinstance(items, list):
            return False
        return any(isinstance(item, dict) and item.get("name") == name for item in items)

    def upsert_template(self) -> str:
        base = f"/api/{self._org()}/alerts/templates"
        payload = {"name": TEMPLATE_NAME, "body": TEMPLATE_BODY, "isDefault": False}
        if self._exists(base, TEMPLATE_NAME):
            self.json_request("PUT", f"{base}/{parse.quote(TEMPLATE_NAME, safe='')}", payload)
            return "updated"
        self.json_request("POST", base, payload)
        return "created"

    def upsert_destination(self) -> str:
        base = f"/api/{self._org()}/alerts/destinations"
        payload = {
            "name": DESTINATION_NAME,
            "url": self.webhook,
            "method": "post",
            "skip_tls_verify": False,
            "template": TEMPLATE_NAME,
            "headers": {},
        }
        if self._exists(base, DESTINATION_NAME):
            self.json_request("PUT", f"{base}/{parse.quote(DESTINATION_NAME, safe='')}", payload)
            return "updated"
        self.json_request("POST", base, payload)
        return "created"

    def upsert_alert(self, alert: dict[str, Any]) -> str:
        stream = parse.quote(self.stream, safe="")
        base = f"/api/{self._org()}/{stream}/alerts"
        if self._exists(base, ALERT_NAME):
            self.json_request("PUT", f"{base}/{parse.quote(ALERT_NAME, safe='')}", alert)
            return "updated"
        self.json_request("POST", base, alert)
        return "created"


def main() -> int:
    client = OpenObserve()
    template_action = client.upsert_template()
    destination_action = client.upsert_destination()
    alert = build_alert(client.stream, client.minutes)
    alert_action = client.upsert_alert(alert)
    print(
        json.dumps(
            {
                "template": {"name": TEMPLATE_NAME, "action": template_action},
                "destination": {"name": DESTINATION_NAME, "action": destination_action},
                "alert": {
                    "name": ALERT_NAME,
                    "action": alert_action,
                    "stream": client.stream,
                    "deadman_minutes": client.minutes,
                },
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"openobserve alerts error: {exc}", file=sys.stderr)
        raise SystemExit(1)
