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
            ("Fast-0", "load", False),
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
        ("Fast-0", "load", 0),
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
        ("Alice (Kelly)", 12.0),
        ("Fast-0", 900.0),
    ]:
        db.log_snapshot(
            trader_name=name,
            balance=500.0,
            portfolio_value=500.0 + pnl,
            pnl=pnl,
            positions={},
        )

    db.activate_runtime(
        [
            ("Alice (Kelly)", "competitor", True),
            ("Fast-0", "load", False),
        ]
    )

    scored = queries.get_latest_snapshots(db.conn)
    assert scored["trader_name"].tolist() == ["Alice (Kelly)"]
    assert scored["pnl"].sum() == 12.0
    assert len(queries.get_latest_snapshots(db.conn, scored_only=False)) == 3
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
    db.close()
