#!/usr/bin/env python3
"""Send the four marked Stage 2 live-gate webhook probes."""

from __future__ import annotations

import json
import os
import sys
import urllib.request
import uuid


def main() -> int:
    url = os.environ.get("STANDX_SUPERVISOR_WEBHOOK", "")
    if not url:
        print("STANDX_SUPERVISOR_WEBHOOK is required", file=sys.stderr)
        return 64
    webhook_format = os.environ.get("STANDX_SUPERVISOR_WEBHOOK_FORMAT", "slack").lower()
    if webhook_format not in {"slack", "telegram", "feishu", "raw"}:
        print(f"unsupported STANDX_SUPERVISOR_WEBHOOK_FORMAT={webhook_format}", file=sys.stderr)
        return 64
    test_id = f"stage2-webhook-{uuid.uuid4().hex[:12]}"
    for kind in ("stop_loss", "position_risk", "equity", "margin"):
        text = f"STANDX STAGE2 WEBHOOK TEST kind={kind} test_id={test_id}"
        raw = {
            "text": text,
            "action": "risk_notification",
            "kind": kind,
            "severity": "critical" if kind in {"stop_loss", "position_risk"} else "warning",
            "test": True,
            "test_id": test_id,
        }
        if webhook_format == "feishu":
            payload = {"msg_type": "text", "content": {"text": text}}
        elif webhook_format == "raw":
            payload = raw
        else:
            payload = {"text": text}
        body = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=5) as response:
            if not 200 <= response.status < 300:
                raise RuntimeError(f"{kind} returned HTTP {response.status}")
        print(f"sent kind={kind} test_id={test_id}")
    print(f"confirm all four received before recording gate evidence: test_id={test_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
