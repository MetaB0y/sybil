import sqlite3

from live.db import DecisionDB


def test_legacy_snapshot_table_migrates_actor_identity(tmp_path):
    path = tmp_path / "decisions.db"
    conn = sqlite3.connect(path)
    conn.execute(
        """CREATE TABLE portfolio_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            timestamp TEXT,
            balance REAL,
            portfolio_value REAL,
            pnl REAL,
            positions TEXT
        )"""
    )
    conn.commit()
    conn.close()

    db = DecisionDB(str(path))
    db.log_snapshot(
        trader_name="LLM Alice",
        balance=90.0,
        portfolio_value=101.0,
        pnl=1.0,
        positions={},
        account_id=17,
        participant_kind="llm",
    )
    row = db.conn.execute(
        "SELECT account_id, participant_kind, total_fills, total_orders "
        "FROM portfolio_snapshots ORDER BY id DESC LIMIT 1"
    ).fetchone()

    assert tuple(row) == (17, "llm", 0, 0)
    db.conn.close()
