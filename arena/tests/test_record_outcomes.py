import sqlite3

import httpx
import pytest

from scripts.record_outcomes import OutcomeConflictError, record_outcomes


def _decisions_db(path):
    conn = sqlite3.connect(path)
    conn.executescript("""
        CREATE TABLE decisions (market_id INTEGER);
        INSERT INTO decisions (market_id) VALUES (1), (1), (2);
    """)
    conn.commit()
    conn.close()


def test_record_outcomes_is_idempotent_and_dry_run_does_not_write(tmp_path):
    db_path = tmp_path / "decisions.db"
    _decisions_db(db_path)

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        if market_id == 1:
            return httpx.Response(
                200,
                json={
                    "market_id": 1,
                    "status": "resolved",
                    "payout_nanos": 1_000_000_000,
                    "resolved_at_ms": 1_767_225_600_000,
                },
            )
        return httpx.Response(200, json={"market_id": 2, "status": "active", "payout_nanos": None})

    transport = httpx.MockTransport(handler)
    dry = record_outcomes(str(db_path), dry_run=True, transport=transport)
    assert dry["would_insert"] == 1
    conn = sqlite3.connect(db_path)
    assert (
        conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
        ).fetchone()
        is None
    )
    conn.close()

    first = record_outcomes(str(db_path), transport=transport)
    second = record_outcomes(str(db_path), transport=transport)
    assert first["inserted"] == 1
    assert second == {
        "markets_seen": 2,
        "resolved": 1,
        "already_recorded": 1,
        "inserted": 0,
    }
    conn = sqlite3.connect(db_path)
    assert conn.execute(
        "SELECT market_id, outcome, resolved_at FROM market_outcomes"
    ).fetchone() == (1, 1.0, "2026-01-01T00:00:00+00:00")
    conn.close()


def test_record_outcomes_raises_on_conflicting_existing_outcome(tmp_path):
    db_path = tmp_path / "decisions.db"
    _decisions_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.execute(
        "CREATE TABLE market_outcomes "
        "(market_id INTEGER PRIMARY KEY, outcome REAL, resolved_at TEXT)"
    )
    conn.execute("INSERT INTO market_outcomes VALUES (1, 0.0, '2026-01-01T00:00:00Z')")
    conn.commit()
    conn.close()

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        if market_id == 1:
            return httpx.Response(
                200,
                json={
                    "market_id": 1,
                    "status": "resolved",
                    "payout_nanos": 1_000_000_000,
                    "resolved_at_ms": 1_767_225_600_000,
                },
            )
        return httpx.Response(404)

    with pytest.raises(OutcomeConflictError, match="outcome conflict"):
        record_outcomes(str(db_path), transport=httpx.MockTransport(handler))

    conn = sqlite3.connect(db_path)
    assert (
        conn.execute("SELECT outcome FROM market_outcomes WHERE market_id = 1").fetchone()[0] == 0
    )
    conn.close()
