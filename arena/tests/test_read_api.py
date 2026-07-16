import json
import sqlite3
import urllib.error
import urllib.request

import pytest

from live.read_api import load_decision_feed, load_equity_series, start_read_server


def _database(path):
    conn = sqlite3.connect(path)
    conn.executescript(
        """
        CREATE TABLE decisions (
            id INTEGER PRIMARY KEY, run_id TEXT, trader_name TEXT NOT NULL,
            market_id INTEGER, market_name TEXT, timestamp TEXT, article_urls TEXT,
            analysis TEXT, fair_value REAL, market_price REAL, orders TEXT,
            motivation TEXT, llm_duration_s REAL, balance REAL, yes_pos REAL, no_pos REAL
        );
        CREATE TABLE portfolio_snapshots (
            id INTEGER PRIMARY KEY, run_id TEXT, trader_name TEXT NOT NULL,
            account_id INTEGER, timestamp TEXT, balance REAL, portfolio_value REAL,
            pnl REAL, total_fills INTEGER, total_orders INTEGER
        );
        CREATE TABLE arena_runs (
            run_id TEXT PRIMARY KEY, started_at_utc TEXT, heartbeat_at_utc TEXT,
            stopped_at_utc TEXT
        );
        CREATE TABLE arena_run_participants (
            run_id TEXT, trader_name TEXT, role TEXT, scored INTEGER
        );
        CREATE TABLE token_usage (
            trader_name TEXT, prompt_tokens INTEGER, completion_tokens INTEGER,
            duration_s REAL, model TEXT
        );
        """
    )
    conn.execute(
        "INSERT INTO arena_runs VALUES ('live', datetime('now'), datetime('now'), NULL)"
    )
    conn.executemany(
        "INSERT INTO arena_run_participants VALUES ('live', ?, ?, ?)",
        [("alice", "competitor", 1), ("load", "load", 1)],
    )
    conn.executemany(
        "INSERT INTO decisions VALUES (?, ?, ?, 7, 'Market', ?, '[]', 'analysis', ?, .4, '[]', 'why', 1, 99, 5, 3)",
        [
            (1, "old", "alice", "2026-07-01T00:00:00Z", .1),
            (2, "live", "alice", "2026-07-02T00:00:00Z", .6),
        ],
    )
    for idx in range(5):
        conn.execute(
            "INSERT INTO portfolio_snapshots VALUES (?, 'live', 'alice', 42, ?, 100, ?, ?, ?, ?)",
            (idx + 1, f"2026-07-0{idx + 1}T00:00:00Z", 100 + idx, idx, idx, idx + 10),
        )
    conn.execute("INSERT INTO token_usage VALUES ('alice', 10, 5, 1.5, 'model')")
    conn.commit()
    conn.close()


def test_decision_feed_is_scoped_to_live_runtime(tmp_path):
    db_path = tmp_path / "arena.db"
    _database(db_path)

    body = load_decision_feed(str(db_path), trader="alice", market_id=7, limit=10)

    assert body["db_available"] is True
    assert len(body["decisions"]) == 2
    alice = next(row for row in body["summaries"] if row["trader_name"] == "alice")
    assert alice["active"] is True
    assert alice["scored"] is True
    assert alice["decision_count"] == 1
    assert alice["account_id"] == 42
    assert alice["portfolio_value"] == 104
    load = next(row for row in body["summaries"] if row["trader_name"] == "load")
    assert load["active"] is True
    assert load["scored"] is False


def test_equity_series_downsamples_and_retains_latest(tmp_path):
    db_path = tmp_path / "arena.db"
    _database(db_path)

    body = load_equity_series(str(db_path), trader="alice", limit=2)

    assert body["source_rows"] == 5
    assert body["stride"] == 3
    assert body["returned_rows"] == 2
    assert [point["portfolio_value"] for point in body["points"]] == [100, 104]


def test_private_http_boundary_requires_bearer_token(tmp_path):
    db_path = tmp_path / "arena.db"
    _database(db_path)
    server, thread = start_read_server(str(db_path), "127.0.0.1", 0, "secret") or (None, None)
    # Port zero is the documented disabled value; bind an ephemeral port directly
    # for the transport/authentication test.
    if server is None:
        from live.read_api import ArenaReadHttpServer

        server = ArenaReadHttpServer(("127.0.0.1", 0), str(db_path), "secret")
        import threading

        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
    url = f"http://127.0.0.1:{server.server_port}/v1/decisions?limit=1"
    try:
        with pytest.raises(urllib.error.HTTPError) as error:
            urllib.request.urlopen(url)
        assert error.value.code == 401
        request = urllib.request.Request(url, headers={"Authorization": "Bearer secret"})
        with urllib.request.urlopen(request) as response:
            body = json.load(response)
        assert body["db_available"] is True
        assert len(body["decisions"]) == 1
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
