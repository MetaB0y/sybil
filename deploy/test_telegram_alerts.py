"""Tests for the vmalert-to-Telegram bridge send accounting (OPS-11).

A transient Telegram delivery failure must not (a) suppress the alert for
REPEAT_SECONDS by recording a send that never happened, nor (b) abort delivery
of the remaining alerts in the same batch.
"""

from __future__ import annotations

import http.client
import importlib.util
import json
import threading
import unittest
from pathlib import Path
from unittest import mock

_MODULE_PATH = Path(__file__).with_name("telegram-alerts.py")
_spec = importlib.util.spec_from_file_location("telegram_alerts", _MODULE_PATH)
assert _spec is not None and _spec.loader is not None
tg = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tg)


def _alert(name: str) -> dict:
    return {"status": "firing", "labels": {"alertname": name}}


class TelegramAlertsTests(unittest.TestCase):
    def setUp(self) -> None:
        tg._last_sent.clear()

    def tearDown(self) -> None:
        tg._last_sent.clear()

    def test_failure_does_not_record_last_sent(self) -> None:
        alert = _alert("BlockProductionStalled")

        with mock.patch.object(
            tg, "_send_telegram", side_effect=RuntimeError("telegram down")
        ):
            self.assertTrue(tg._should_send(alert))
            with self.assertRaises(RuntimeError):
                tg._send_telegram(tg._format_alert(alert))

        key = (tg._alert_key(alert), tg._alert_status(alert))
        self.assertNotIn(key, tg._last_sent)
        self.assertTrue(tg._should_send(alert))

    def test_one_failure_does_not_abort_remaining_alerts(self) -> None:
        alerts = [_alert("FirstAlert"), _alert("SecondAlert")]
        attempted: list[str] = []

        def record_and_maybe_fail(message: str) -> None:
            attempted.append(message)
            if "FirstAlert" in message:
                raise RuntimeError("transient failure on first alert")

        with mock.patch.object(tg, "_send_telegram", side_effect=record_and_maybe_fail):
            sent, skipped, failed = tg._deliver_alerts(alerts)

        self.assertTrue(any("SecondAlert" in message for message in attempted))
        self.assertEqual((sent, skipped, failed), (1, 0, 1))
        first_key = (tg._alert_key(alerts[0]), tg._alert_status(alerts[0]))
        second_key = (tg._alert_key(alerts[1]), tg._alert_status(alerts[1]))
        self.assertNotIn(first_key, tg._last_sent)
        self.assertIn(second_key, tg._last_sent)

    def test_http_failure_is_retryable_and_visible_to_vmalert(self) -> None:
        attempts = 0

        def fail_once(_message: str) -> None:
            nonlocal attempts
            attempts += 1
            if attempts == 1:
                raise RuntimeError("telegram temporarily unavailable")

        server = tg.ThreadingHTTPServer(("127.0.0.1", 0), tg.Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            payload = json.dumps([_alert("RetryMe")])
            statuses = []
            bodies = []
            with mock.patch.object(tg, "_send_telegram", side_effect=fail_once):
                for _ in range(2):
                    connection = http.client.HTTPConnection(
                        *server.server_address, timeout=2
                    )
                    connection.request(
                        "POST",
                        "/api/v2/alerts",
                        body=payload,
                        headers={"Content-Type": "application/json"},
                    )
                    response = connection.getresponse()
                    statuses.append(response.status)
                    bodies.append(response.read().decode())
                    connection.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

        self.assertEqual(statuses, [502, 200])
        self.assertIn("failed=1", bodies[0])
        self.assertIn("sent=1", bodies[1])
        self.assertEqual(attempts, 2)

    def test_json_payload_shape(self) -> None:
        message = tg._format_alert(_alert("SanityCheck"))
        self.assertIn("SanityCheck", message)


if __name__ == "__main__":
    unittest.main()
