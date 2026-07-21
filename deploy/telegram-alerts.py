#!/usr/bin/env python3
"""Minimal vmalert-to-Telegram notification bridge.

vmalert sends Prometheus Alertmanager v2 payloads to /api/v2/alerts. This
service accepts that shape and forwards concise messages to Telegram.
"""

from __future__ import annotations

import html
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any


HOST = os.environ.get("TELEGRAM_ALERT_HOST", "0.0.0.0")
PORT = int(os.environ.get("TELEGRAM_ALERT_PORT", "8080"))
BOT_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
CHAT_ID = os.environ.get("TELEGRAM_CHAT_ID", "")
PREFIX = os.environ.get("TELEGRAM_ALERT_PREFIX", "Sybil")
REPEAT_SECONDS = int(os.environ.get("TELEGRAM_ALERT_REPEAT_SECONDS", "1800"))
MAX_MESSAGE_CHARS = 3900

_last_sent: dict[tuple[str, str], float] = {}


def _alert_key(alert: dict[str, Any]) -> str:
    labels = alert.get("labels", {})
    return "|".join(f"{k}={labels[k]}" for k in sorted(labels))


def _alert_status(alert: dict[str, Any]) -> str:
    status = str(alert.get("status") or "").lower()
    if status in {"resolved", "firing"}:
        return status

    ends_at = str(alert.get("endsAt") or "")
    if not ends_at or ends_at.startswith("0001-01-01"):
        return "firing"

    try:
        expires = datetime.fromisoformat(ends_at.replace("Z", "+00:00"))
    except ValueError:
        return "firing"

    if expires <= datetime.now(timezone.utc):
        return "resolved"
    return "firing"


def _should_send(alert: dict[str, Any]) -> bool:
    status = _alert_status(alert)
    key = (_alert_key(alert), status)
    now = time.time()
    previous = _last_sent.get(key)
    if previous is not None and now - previous < REPEAT_SECONDS:
        return False
    return True


def _record_sent(alert: dict[str, Any]) -> None:
    status = _alert_status(alert)
    key = (_alert_key(alert), status)
    _last_sent[key] = time.time()


def _fmt_label(name: str, labels: dict[str, Any]) -> str:
    value = labels.get(name)
    return f"{name}={html.escape(str(value))}" if value else ""


def _format_alert(alert: dict[str, Any]) -> str:
    labels = alert.get("labels", {})
    annotations = alert.get("annotations", {})
    status = _alert_status(alert)
    icon = "RESOLVED" if status == "resolved" else "FIRING"
    alert_name = html.escape(str(labels.get("alertname", "alert")))
    severity = html.escape(str(labels.get("severity", "unknown")))
    summary = html.escape(str(annotations.get("summary", "")))
    description = html.escape(str(annotations.get("description", "")))

    context = [
        _fmt_label("component", labels),
        _fmt_label("instance", labels),
        _fmt_label("job", labels),
    ]
    context_line = " ".join(item for item in context if item)

    lines = [
        f"<b>{html.escape(PREFIX)} {icon}</b>",
        f"<b>{alert_name}</b> severity={severity}",
    ]
    if context_line:
        lines.append(context_line)
    if summary:
        lines.append("")
        lines.append(summary)
    if description:
        lines.append(description)

    message = "\n".join(lines)
    if len(message) > MAX_MESSAGE_CHARS:
        return message[: MAX_MESSAGE_CHARS - 1] + "..."
    return message


def _send_telegram(message: str) -> None:
    if not BOT_TOKEN or not CHAT_ID:
        raise RuntimeError("TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID are required")

    url = f"https://api.telegram.org/bot{BOT_TOKEN}/sendMessage"
    body = urllib.parse.urlencode(
        {
            "chat_id": CHAT_ID,
            "text": message,
            "parse_mode": "HTML",
            "disable_web_page_preview": "true",
        }
    ).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    with urllib.request.urlopen(req, timeout=15) as response:
        if response.status >= 300:
            raise RuntimeError(f"Telegram returned HTTP {response.status}")


def _deliver_alerts(alerts: list[Any]) -> tuple[int, int, int]:
    sent = 0
    skipped = 0
    failed = 0
    for alert in alerts:
        if not isinstance(alert, dict):
            skipped += 1
            continue
        if not _should_send(alert):
            skipped += 1
            continue
        try:
            _send_telegram(_format_alert(alert))
        except (urllib.error.URLError, RuntimeError) as exc:
            # A transient delivery failure must not suppress this alert
            # (don't record _last_sent) nor abort the rest of the batch.
            failed += 1
            print(f"telegram alert delivery failed: {exc}", flush=True)
            continue
        _record_sent(alert)
        sent += 1
    return sent, skipped, failed


class Handler(BaseHTTPRequestHandler):
    server_version = "sybil-telegram-alerts/1.0"

    def log_message(self, fmt: str, *args: Any) -> None:
        print(f"{self.address_string()} - {fmt % args}", flush=True)

    def _write(self, status: int, body: str) -> None:
        payload = body.encode()
        self.send_response(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self) -> None:
        if self.path in {"/", "/health", "/-/healthy"}:
            self._write(200, "ok\n")
        else:
            self._write(404, "not found\n")

    def do_POST(self) -> None:
        if self.path != "/api/v2/alerts":
            self._write(404, "not found\n")
            return

        try:
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length)
            alerts = json.loads(raw.decode() or "[]")
            if not isinstance(alerts, list):
                raise ValueError("payload must be a JSON list")

            sent, skipped, failed = _deliver_alerts(alerts)
            # Tell vmalert that at least one notification was not delivered.
            # It will retry the batch; already-sent siblings are deduplicated,
            # while failed alerts were deliberately not recorded above.
            status = 502 if failed else 200
            self._write(status, f"sent={sent} skipped={skipped} failed={failed}\n")
        except (ValueError, json.JSONDecodeError) as exc:
            self._write(400, f"invalid alert payload: {exc}\n")


def main() -> None:
    if not BOT_TOKEN or not CHAT_ID:
        raise SystemExit("TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID are required")
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"telegram-alerts listening on {HOST}:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
