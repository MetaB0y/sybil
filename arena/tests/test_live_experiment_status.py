"""Experiment-window visibility in the plain-text Arena status path."""

import json
import sqlite3
from datetime import datetime, timezone
from hashlib import sha256

from live.queries import get_live_experiment_status
from live.personas import PERSONAS


def _db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.executescript(
        """
        CREATE TABLE live_experiments (
            experiment_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL,
            started_at_utc TEXT NOT NULL,
            configuration_json TEXT NOT NULL
        );
        CREATE TABLE decisions (trader_name TEXT, timestamp TEXT);
        CREATE TABLE portfolio_snapshots (trader_name TEXT, timestamp TEXT);
        """
    )
    return conn


def test_experiment_status_requires_exact_durable_arm_activity():
    conn = _db()
    config = {
        "personas": ["news_trader", "contrarian"],
        "market_ids": [7, 11],
        "model": "test/model",
        "persona_display_name_sha256": {
            persona_key: sha256(PERSONAS[persona_key]["name"].encode()).hexdigest()
            for persona_key in ("news_trader", "contrarian")
        },
    }
    conn.execute(
        "INSERT INTO live_experiments VALUES (?, ?, ?, ?)",
        (
            "stage1-july",
            "syb-114-stage1-ab",
            "2026-07-12T01:00:00+00:00",
            json.dumps(config),
        ),
    )
    exact_names = [
        "News Trader [SYB-114:stage1-july:control] (Flat)",
        "Contrarian [SYB-114:stage1-july:control] (Flat)",
        "News Trader [SYB-114:stage1-july:stage1] (Flat)",
    ]
    for index, name in enumerate(exact_names):
        conn.execute(
            "INSERT INTO decisions VALUES (?, ?)",
            (name, f"2026-07-12T01:0{index}:00+00:00"),
        )
        conn.execute(
            "INSERT INTO portfolio_snapshots VALUES (?, ?)",
            (name, f"2026-07-12T01:0{index}:30+00:00"),
        )
    # Similar-looking ordinary and analyst identities must not count.
    conn.execute(
        "INSERT INTO decisions VALUES (?, ?)",
        ("News Trader (Flat)", "2026-07-12T01:04:00+00:00"),
    )
    conn.execute(
        "INSERT INTO decisions VALUES (?, ?)",
        (
            "Contrarian [SYB-114:stage1-july:stage1] (Analyst)",
            "2026-07-12T01:05:00+00:00",
        ),
    )
    conn.execute(
        "INSERT INTO decisions VALUES (?, ?)",
        (
            "Impostor [SYB-114:stage1-july:stage1] (Flat)",
            "2026-07-12T01:06:00+00:00",
        ),
    )
    conn.execute(
        "INSERT INTO portfolio_snapshots VALUES (?, ?)",
        (
            "Different Impostor [SYB-114:stage1-july:stage1] (Flat)",
            "2026-07-12T01:06:30+00:00",
        ),
    )

    [status] = get_live_experiment_status(conn)

    assert status["experiment_id"] == "stage1-july"
    assert status["expected_traders_per_arm"] == 2
    assert status["arms"]["control"] == {
        "decision_count": 2,
        "decision_traders": 2,
        "first_decision_at": "2026-07-12T01:00:00+00:00",
        "last_decision_at": "2026-07-12T01:01:00+00:00",
        "snapshot_count": 2,
        "snapshot_traders": 2,
        "first_snapshot_at": "2026-07-12T01:00:30+00:00",
        "last_snapshot_at": "2026-07-12T01:01:30+00:00",
        "ready": True,
    }
    assert status["arms"]["stage1"]["decision_traders"] == 1
    assert status["arms"]["stage1"]["snapshot_traders"] == 1
    assert status["arms"]["stage1"]["ready"] is False


def test_experiment_status_fails_closed_on_identity_metadata_drift():
    conn = _db()
    conn.execute(
        "INSERT INTO live_experiments VALUES (?, ?, ?, ?)",
        (
            "exp",
            "syb-114-stage1-ab",
            "2026-07-12T01:00:00+00:00",
            json.dumps(
                {
                    "personas": ["news_trader"],
                    "market_ids": [7],
                    "persona_display_name_sha256": {"news_trader": "0" * 64},
                }
            ),
        ),
    )
    conn.execute(
        "INSERT INTO decisions VALUES (?, ?)",
        (
            "News Trader [SYB-114:exp:control] (Flat)",
            "2026-07-12T01:01:00+00:00",
        ),
    )

    [status] = get_live_experiment_status(conn)

    assert status["identity_error"] == "display-name fingerprint drift: 'news_trader'"
    assert status["expected_traders_per_arm"] == 0
    assert status["arms"]["control"]["decision_count"] == 0
    assert status["arms"]["control"]["ready"] is False


def test_experiment_status_is_backward_compatible_with_old_database():
    conn = sqlite3.connect(":memory:")
    conn.execute("CREATE TABLE decisions (trader_name TEXT, timestamp TEXT)")

    assert get_live_experiment_status(conn) == []


def test_experiment_start_is_parseable_for_24_hour_window():
    conn = _db()
    conn.execute(
        "INSERT INTO live_experiments VALUES (?, ?, ?, ?)",
        (
            "exp",
            "syb-114-stage1-ab",
            "2026-07-12T01:00:00+00:00",
            json.dumps(
                {
                    "personas": ["news_trader"],
                    "market_ids": [7],
                    "persona_display_name_sha256": {
                        "news_trader": sha256(
                            PERSONAS["news_trader"]["name"].encode()
                        ).hexdigest()
                    },
                }
            ),
        ),
    )

    [status] = get_live_experiment_status(conn)
    started = datetime.fromisoformat(status["started_at_utc"])

    assert started == datetime(2026, 7, 12, 1, tzinfo=timezone.utc)
