import sqlite3

import pytest

from live import queries
from live.db import DecisionDB


def test_runtime_activation_replaces_live_membership_and_preserves_history(tmp_path):
    db = DecisionDB(str(tmp_path / "arena.db"))

    first = db.activate_runtime(
        [
            ("Alice (Kelly)", "competitor", True),
            ("Noise-0", "noise", False),
        ]
    )
    second = db.activate_runtime(
        [
            ("Alice (Kelly)", "competitor", True),
            ("Load (Kelly)", "load", False),
        ]
    )

    first_row = db.conn.execute(
        "SELECT stopped_at_utc FROM arena_runs WHERE run_id = ?", (first,)
    ).fetchone()
    second_row = db.conn.execute(
        "SELECT stopped_at_utc FROM arena_runs WHERE run_id = ?", (second,)
    ).fetchone()
    assert first_row[0] is not None
    assert second_row[0] is None
    with pytest.raises(RuntimeError, match="no longer active"):
        db.heartbeat_runtime(first)
    assert (
        db.conn.execute(
            "SELECT COUNT(*) FROM arena_run_participants WHERE run_id = ?", (first,)
        ).fetchone()[0]
        == 2
    )
    participant_rows = db.conn.execute(
        "SELECT trader_name, role, scored FROM arena_run_participants "
        "WHERE run_id = ? ORDER BY trader_name",
        (second,),
    ).fetchall()
    assert [tuple(row) for row in participant_rows] == [
        ("Alice (Kelly)", "competitor", 1),
        ("Load (Kelly)", "load", 0),
    ]

    db.stop_runtime(second)
    assert (
        db.conn.execute(
            "SELECT stopped_at_utc FROM arena_runs WHERE run_id = ?", (second,)
        ).fetchone()[0]
        is not None
    )
    db.close()


def test_latest_snapshots_scores_only_the_active_runtime(tmp_path):
    db = DecisionDB(str(tmp_path / "arena.db"))
    for name, pnl in [
        ("Old (Kelly)", 200.0),
        ("Alice (Kelly)", 700.0),
    ]:
        db.log_snapshot(
            trader_name=name,
            balance=500.0,
            portfolio_value=500.0 + pnl,
            pnl=pnl,
            positions={},
        )

    run_id = db.activate_runtime(
        [
            ("Alice (Kelly)", "competitor", True),
            ("Load (Kelly)", "load", False),
        ]
    )
    db.log_snapshot(
        trader_name="Alice (Kelly)",
        balance=500.0,
        portfolio_value=512.0,
        pnl=12.0,
        positions={},
    )
    db.log_snapshot(
        trader_name="Load (Kelly)",
        balance=500.0,
        portfolio_value=1_400.0,
        pnl=900.0,
        positions={},
    )
    db.conn.executemany(
        "INSERT INTO decisions (run_id, trader_name, fair_value, market_price) VALUES (?, ?, ?, ?)",
        [
            (run_id, "Alice (Kelly)", 0.6, 0.5),
            (run_id, "Load (Kelly)", 0.95, 0.05),
        ],
    )
    db.conn.commit()

    scored = queries.get_latest_snapshots(db.conn)
    assert scored["trader_name"].tolist() == ["Alice (Kelly)"]
    assert scored["pnl"].sum() == 12.0
    assert (
        db.conn.execute(
            "SELECT run_id FROM portfolio_snapshots ORDER BY id DESC LIMIT 1"
        ).fetchone()[0]
        == run_id
    )
    assert len(queries.get_latest_snapshots(db.conn, scored_only=False)) == 3
    strategies = queries.get_strategy_comparison(db.conn)
    assert strategies is not None
    assert strategies["traders"].tolist() == [1]
    assert strategies["total_pnl"].tolist() == [12.0]
    assert strategies["avg_edge"].tolist() == pytest.approx([0.1])
    db.close()


def test_runtime_replacement_rolls_back_as_one_transaction(tmp_path):
    db = DecisionDB(str(tmp_path / "arena.db"))
    first = db.activate_runtime([("Alice (Kelly)", "competitor", True)])
    db.conn.execute(
        """CREATE TRIGGER reject_broken_runtime_participant
           BEFORE INSERT ON arena_run_participants
           WHEN NEW.trader_name = 'broken'
           BEGIN SELECT RAISE(ABORT, 'synthetic participant failure'); END"""
    )
    db.conn.commit()

    with pytest.raises(sqlite3.IntegrityError, match="synthetic participant failure"):
        db.activate_runtime([("broken", "competitor", True)])

    rows = db.conn.execute(
        "SELECT run_id, stopped_at_utc FROM arena_runs ORDER BY started_at_utc"
    ).fetchall()
    assert [tuple(row) for row in rows] == [(first, None)]
    db.log_snapshot("Alice (Kelly)", 100.0, 100.0, 0.0, {})
    assert (
        db.conn.execute(
            "SELECT run_id FROM portfolio_snapshots ORDER BY id DESC LIMIT 1"
        ).fetchone()[0]
        == first
    )
    db.close()


def test_runtime_activation_rejects_ambiguous_membership(tmp_path):
    db = DecisionDB(str(tmp_path / "arena.db"))
    try:
        db.activate_runtime(
            [
                ("duplicate", "competitor", True),
                ("duplicate", "load", False),
            ]
        )
    except ValueError as error:
        assert "unique" in str(error)
    else:
        raise AssertionError("duplicate runtime membership must fail closed")

    with pytest.raises(ValueError, match="cannot be scored"):
        db.activate_runtime([("load", "load", True)])
    with pytest.raises(ValueError, match="competitor, load, or noise"):
        db.activate_runtime([("unknown", "observer", False)])
    db.close()
