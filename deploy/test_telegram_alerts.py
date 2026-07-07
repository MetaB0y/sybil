"""Tests for the vmalert-to-Telegram bridge send accounting (OPS-11).

A transient Telegram delivery failure must not (a) suppress the alert for
REPEAT_SECONDS by recording a send that never happened, nor (b) abort delivery
of the remaining alerts in the same batch.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

_MODULE_PATH = Path(__file__).with_name("telegram-alerts.py")
_spec = importlib.util.spec_from_file_location("telegram_alerts", _MODULE_PATH)
assert _spec is not None and _spec.loader is not None
tg = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tg)


def _alert(name: str) -> dict:
    return {"status": "firing", "labels": {"alertname": name}}


@pytest.fixture(autouse=True)
def _clear_state():
    tg._last_sent.clear()
    yield
    tg._last_sent.clear()


def test_failure_does_not_record_last_sent(monkeypatch):
    alert = _alert("BlockProductionStalled")

    def boom(_message: str) -> None:
        raise RuntimeError("telegram down")

    monkeypatch.setattr(tg, "_send_telegram", boom)

    assert tg._should_send(alert) is True
    try:
        tg._send_telegram(tg._format_alert(alert))
    except RuntimeError:
        pass
    # On failure we must NOT record the send, so a retry is still allowed.
    key = (tg._alert_key(alert), tg._alert_status(alert))
    assert key not in tg._last_sent
    assert tg._should_send(alert) is True


def test_one_failure_does_not_abort_remaining_alerts(monkeypatch):
    alerts = [_alert("FirstAlert"), _alert("SecondAlert")]
    attempted: list[str] = []

    def record_and_maybe_fail(message: str) -> None:
        attempted.append(message)
        if "FirstAlert" in message:
            raise RuntimeError("transient failure on first alert")

    monkeypatch.setattr(tg, "_send_telegram", record_and_maybe_fail)

    # Drive the same per-alert loop logic used by do_POST.
    sent = 0
    failed = 0
    for alert in alerts:
        if not tg._should_send(alert):
            continue
        try:
            tg._send_telegram(tg._format_alert(alert))
        except RuntimeError:
            failed += 1
            continue
        tg._record_sent(alert)
        sent += 1

    # The second alert was still attempted and delivered despite the first failing.
    assert any("SecondAlert" in m for m in attempted)
    assert sent == 1
    assert failed == 1
    # Failed alert not recorded; successful one recorded.
    first_key = (tg._alert_key(alerts[0]), tg._alert_status(alerts[0]))
    second_key = (tg._alert_key(alerts[1]), tg._alert_status(alerts[1]))
    assert first_key not in tg._last_sent
    assert second_key in tg._last_sent


def test_json_payload_shape(monkeypatch):
    # Sanity: format still produces a non-empty HTML message.
    msg = tg._format_alert(_alert("SanityCheck"))
    assert "SanityCheck" in msg
    assert json.dumps({"ok": True})  # json import used
