import json
import sqlite3
import sys

import pytest

import scripts.calibration_compare as comparison_module
from scripts.calibration_compare import compare_decisions_dbs, format_delta_table, main


def _comparison_db(path, *, outcomes, decisions, snapshots=()):
    conn = sqlite3.connect(path)
    conn.executescript("""
        CREATE TABLE market_outcomes (
            market_id INTEGER PRIMARY KEY,
            outcome REAL NOT NULL
        );
        CREATE TABLE decisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            market_id INTEGER,
            market_name TEXT,
            timestamp TEXT,
            fair_value REAL,
            market_price REAL,
            orders TEXT
        );
        CREATE TABLE portfolio_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            timestamp TEXT,
            balance REAL,
            portfolio_value REAL,
            pnl REAL,
            positions TEXT
        );
    """)
    conn.executemany("INSERT INTO market_outcomes (market_id, outcome) VALUES (?, ?)", outcomes)
    conn.executemany(
        """INSERT INTO decisions
           (trader_name, market_id, market_name, timestamp, fair_value, market_price, orders)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        decisions,
    )
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, ?, 0, 0, ?, '{}')""",
        snapshots,
    )
    conn.commit()
    conn.close()


def _valid_pair(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    _comparison_db(
        before,
        outcomes=[(1, 1.0)],
        decisions=[
            ("Alice (Flat)", 1, "M1", "2026-01-01T00:00:00Z", 0.8, 0.7, "[{}]"),
            ("Alice (Flat)", 2, "M2", "2026-01-01T12:00:00Z", 0.4, 0.3, "[]"),
            # The half-open end must not leak into the discovered cohort.
            ("Alice (Flat)", 4, "M4", "2026-01-02T00:00:00Z", 0.5, 0.5, "[]"),
        ],
        snapshots=[
            ("Alice (Flat)", "2025-12-31T23:00:00Z", 0.0),
            ("Alice (Flat)", "2026-01-01T12:00:00Z", 2.0),
        ],
    )
    _comparison_db(
        after,
        outcomes=[(1, 1.0)],
        decisions=[
            ("Alice (Flat)", 1, "M1", "2026-02-01T00:00:00Z", 0.9, 0.8, "[{}]"),
            ("Alice (Flat)", 2, "M2", "2026-02-01T12:00:00Z", 0.6, 0.4, "[]"),
            ("Alice (Flat)", 5, "M5", "2026-02-01T12:00:00Z", 0.6, 0.4, "[]"),
            ("Alice (Flat)", 4, "M4", "2026-02-02T00:00:00Z", 0.5, 0.5, "[]"),
        ],
        snapshots=[
            ("Alice (Flat)", "2026-01-31T23:00:00Z", 5.0),
            ("Alice (Flat)", "2026-02-01T12:00:00Z", 8.0),
        ],
    )
    args = {
        "before_db": str(before),
        "before_since": "2026-01-01T00:00:00Z",
        "before_until": "2026-01-02T00:00:00Z",
        "after_db": str(after),
        "after_since": "2026-02-01T00:00:00Z",
        "after_until": "2026-02-02T00:00:00Z",
    }
    return before, after, args


def test_compare_uses_exact_shared_scoreable_cohort_and_whole_account_pnl(tmp_path):
    _before, _after, args = _valid_pair(tmp_path)

    result = compare_decisions_dbs(**args, bins=2)

    assert result["cohort"] == {
        "before_window_market_ids": [1, 2],
        "after_window_market_ids": [1, 2, 5],
        "shared_available_market_ids": [1, 2],
        "shared_resolved_market_ids": [1],
        "shared_scoreable_market_ids": [1],
        "excluded_shared_market_ids": [2],
    }
    assert result["before"]["cohort"]["requested_market_ids"] == [1]
    assert result["after"]["cohort"]["requested_market_ids"] == [1]
    assert result["before"]["overall"]["brier"] == pytest.approx(0.04)
    assert result["after"]["overall"]["brier"] == pytest.approx(0.01)
    assert result["deltas"]["overall"]["brier"]["delta"] == pytest.approx(-0.03)
    assert result["deltas"]["portfolio_pnl"]["flat"]["mean_pnl"] == {
        "before": 2.0,
        "after": 3.0,
        "delta": 1.0,
    }
    assert result["portfolio_identity_matching"]["flat"] == {
        "matched_trader_names": ["Alice (Flat)"],
        "excluded_before_trader_names": [],
        "excluded_after_trader_names": [],
        "by_trader": {
            "Alice (Flat)": {"before": 2.0, "after": 3.0, "delta": 1.0}
        },
        "matched_mean_pnl": {"before": 2.0, "after": 3.0, "delta": 1.0},
    }
    assert result["portfolio_pnl_scope"] == "all_trader_positions"
    assert "not filtered to the shared market cohort" in result["portfolio_pnl_note"]
    assert result["measurement_protocol"] == {
        "window_semantics": "since inclusive, until exclusive",
        "protocol_default_min_window_hours": 24.0,
        "configured_min_window_hours": 24.0,
        "minimum_window_override": False,
        "exploratory_minimum_override": False,
        "before_window_duration_hours": 24.0,
        "after_window_duration_hours": 24.0,
        "before_outcome_source": "explicit",
        "after_outcome_source": "explicit",
        "allow_inferred_outcomes": False,
        "inferred_outcomes_override_used": False,
    }

    table = format_delta_table(result)
    assert "Arena calibration delta (after - before)" in table
    assert "Windows: before=24h, after=24h, minimum=24h" in table
    assert (
        "Outcome sources: before=explicit, after=explicit, allow inferred=false, override used=false"
        in table
    )
    assert "Shared available market IDs: 1,2" in table
    assert "Shared scoreable market IDs: 1" in table
    assert "Matched Flat trader identities: Alice (Flat)" in table
    assert "Portfolio PnL is whole-account scope" in table
    assert "overall Brier" in table


def test_compare_runs_analyzer_on_discovered_intersection_then_exact_scoreable_set(
    tmp_path, monkeypatch
):
    _before, _after, args = _valid_pair(tmp_path)
    calls = []
    real_analyze = comparison_module.analyze_decisions_db

    def recording_analyze(*call_args, **call_kwargs):
        calls.append(frozenset(call_kwargs["market_ids"]))
        return real_analyze(*call_args, **call_kwargs)

    monkeypatch.setattr(comparison_module, "analyze_decisions_db", recording_analyze)

    compare_decisions_dbs(**args)

    assert calls == [frozenset({1, 2}), frozenset({1, 2}), frozenset({1}), frozenset({1})]


@pytest.mark.parametrize(
    ("overrides", "message"),
    [
        (
            {"before_since": "2026-01-02T00:00:00Z"},
            "before window since must be earlier than until",
        ),
        (
            {"before_since": "", "before_until": "2026-01-02T00:00:00Z"},
            "before window requires explicit",
        ),
        (
            {
                "before_since": "2026-01-03T00:00:00Z",
                "before_until": "2026-01-04T00:00:00Z",
            },
            "before window contains no decisions",
        ),
    ],
)
def test_compare_rejects_invalid_or_empty_windows(tmp_path, overrides, message):
    _before, _after, args = _valid_pair(tmp_path)
    args.update(overrides)

    with pytest.raises(ValueError, match=message):
        compare_decisions_dbs(**args)


def test_compare_requires_protocol_window_unless_minimum_is_explicitly_lowered(tmp_path):
    before, after, args = _valid_pair(tmp_path)
    args["before_until"] = "2026-01-01T06:00:00Z"
    args["after_until"] = "2026-02-01T06:00:00Z"

    with pytest.raises(ValueError, match="below the configured 24h minimum"):
        compare_decisions_dbs(**args)

    for db_path, timestamp, pnl in (
        (before, "2026-01-01T03:00:00Z", 1.0),
        (after, "2026-02-01T03:00:00Z", 6.0),
    ):
        conn = sqlite3.connect(db_path)
        conn.execute(
            """INSERT INTO portfolio_snapshots
               (trader_name, timestamp, balance, portfolio_value, pnl, positions)
               VALUES ('Alice (Flat)', ?, 0, 0, ?, '{}')""",
            (timestamp, pnl),
        )
        conn.commit()
        conn.close()

    result = compare_decisions_dbs(**args, min_window_hours=5)
    assert result["measurement_protocol"] == {
        "window_semantics": "since inclusive, until exclusive",
        "protocol_default_min_window_hours": 24.0,
        "configured_min_window_hours": 5.0,
        "minimum_window_override": True,
        "exploratory_minimum_override": True,
        "before_window_duration_hours": 6.0,
        "after_window_duration_hours": 6.0,
        "before_outcome_source": "explicit",
        "after_outcome_source": "explicit",
        "allow_inferred_outcomes": False,
        "inferred_outcomes_override_used": False,
    }


def test_compare_rejects_empty_market_intersection(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    _comparison_db(
        before,
        outcomes=[(1, 1.0)],
        decisions=[("Alice", 1, "M1", "2026-01-01T00:00:00Z", 0.8, 0.7, "[]")],
    )
    _comparison_db(
        after,
        outcomes=[(2, 0.0)],
        decisions=[("Alice", 2, "M2", "2026-01-01T00:00:00Z", 0.2, 0.3, "[]")],
    )

    with pytest.raises(ValueError, match="no market IDs in common"):
        compare_decisions_dbs(
            before_db=str(before),
            before_since="2026-01-01T00:00:00Z",
            before_until="2026-01-02T00:00:00Z",
            after_db=str(after),
            after_since="2026-01-01T00:00:00Z",
            after_until="2026-01-02T00:00:00Z",
        )


def test_compare_rejects_shared_markets_without_outcomes(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    decision = ("Alice", 1, "M1", "2026-01-01T00:00:00Z", 0.6, 0.5, "[]")
    _comparison_db(before, outcomes=[], decisions=[decision])
    _comparison_db(after, outcomes=[], decisions=[decision])

    with pytest.raises(ValueError, match="no resolved outcomes"):
        compare_decisions_dbs(
            before_db=str(before),
            before_since="2026-01-01T00:00:00Z",
            before_until="2026-01-02T00:00:00Z",
            after_db=str(after),
            after_since="2026-01-01T00:00:00Z",
            after_until="2026-01-02T00:00:00Z",
            allow_inferred_outcomes=True,
        )


def test_compare_requires_explicit_outcomes_unless_exploratory_override_is_recorded(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    decision = ("Alice", 1, "M1", "2026-01-01T00:00:00Z", 0.8, 0.99, "[]")
    snapshots = [
        ("Alice (Flat)", "2025-12-31T23:00:00Z", 0.0),
        ("Alice (Flat)", "2026-01-01T12:00:00Z", 1.0),
    ]
    _comparison_db(before, outcomes=[(1, 1.0)], decisions=[decision], snapshots=snapshots)
    _comparison_db(after, outcomes=[], decisions=[decision], snapshots=snapshots)
    args = {
        "before_db": str(before),
        "before_since": "2026-01-01T00:00:00Z",
        "before_until": "2026-01-02T00:00:00Z",
        "after_db": str(after),
        "after_since": "2026-01-01T00:00:00Z",
        "after_until": "2026-01-02T00:00:00Z",
    }

    with pytest.raises(
        ValueError,
        match="copy the same authoritative market_outcomes labels into working copies",
    ):
        compare_decisions_dbs(**args)

    result = compare_decisions_dbs(**args, allow_inferred_outcomes=True)
    assert result["measurement_protocol"]["before_outcome_source"] == "explicit"
    assert result["measurement_protocol"]["after_outcome_source"] == "final_price_inferred"
    assert result["measurement_protocol"]["allow_inferred_outcomes"] is True
    assert result["measurement_protocol"]["inferred_outcomes_override_used"] is True
    table = format_delta_table(result)
    assert "allow inferred=true, override used=true" in table


def test_compare_excludes_changed_accounts_from_matched_pnl_mean(tmp_path):
    _before, after, args = _valid_pair(tmp_path)
    conn = sqlite3.connect(after)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES ('Changed Cohort (Flat)', ?, 0, 0, ?, '{}')""",
        [
            ("2026-01-31T23:00:00Z", 0.0),
            ("2026-02-01T12:00:00Z", 100.0),
        ],
    )
    conn.commit()
    conn.close()

    result = compare_decisions_dbs(**args)

    matching = result["portfolio_identity_matching"]["flat"]
    assert matching["matched_trader_names"] == ["Alice (Flat)"]
    assert matching["excluded_after_trader_names"] == ["Changed Cohort (Flat)"]
    assert result["deltas"]["portfolio_pnl"]["flat"]["mean_pnl"] == {
        "before": 2.0,
        "after": 3.0,
        "delta": 1.0,
    }


def test_compare_rejects_changed_cohort_with_no_matching_flat_identity(tmp_path):
    before, after, args = _valid_pair(tmp_path)
    conn = sqlite3.connect(before)
    conn.execute(
        "UPDATE portfolio_snapshots SET trader_name = 'Old Cohort (Flat)' "
        "WHERE trader_name = 'Alice (Flat)'"
    )
    conn.commit()
    conn.close()
    conn = sqlite3.connect(after)
    conn.execute(
        "UPDATE portfolio_snapshots SET trader_name = 'New Cohort (Flat)' "
        "WHERE trader_name = 'Alice (Flat)'"
    )
    conn.commit()
    conn.close()

    with pytest.raises(ValueError, match="no matched durable trader identities.*cohort"):
        compare_decisions_dbs(**args)


def test_compare_rejects_resolved_markets_without_scoreable_rows(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    before_decision = ("Alice", 1, "M1", "2026-01-01T00:00:00Z", None, 0.5, "[]")
    after_decision = ("Alice", 1, "M1", "2026-01-01T00:00:00Z", 0.6, None, "[]")
    _comparison_db(before, outcomes=[(1, 1.0)], decisions=[before_decision])
    _comparison_db(after, outcomes=[(1, 1.0)], decisions=[after_decision])

    with pytest.raises(ValueError, match="no scoreable outcomes"):
        compare_decisions_dbs(
            before_db=str(before),
            before_since="2026-01-01T00:00:00Z",
            before_until="2026-01-02T00:00:00Z",
            after_db=str(after),
            after_since="2026-01-01T00:00:00Z",
            after_until="2026-01-02T00:00:00Z",
        )


def test_compare_rejects_conflicting_outcome_labels(tmp_path):
    before = tmp_path / "before.db"
    after = tmp_path / "after.db"
    decision = ("Alice", 1, "M1", "2026-01-01T00:00:00Z", 0.6, 0.5, "[]")
    _comparison_db(before, outcomes=[(1, 1.0)], decisions=[decision])
    _comparison_db(after, outcomes=[(1, 0.0)], decisions=[decision])

    with pytest.raises(ValueError, match="conflicting resolved outcomes"):
        compare_decisions_dbs(
            before_db=str(before),
            before_since="2026-01-01T00:00:00Z",
            before_until="2026-01-02T00:00:00Z",
            after_db=str(after),
            after_since="2026-01-01T00:00:00Z",
            after_until="2026-01-02T00:00:00Z",
        )


def test_command_emits_delta_table_and_json(tmp_path, monkeypatch, capsys):
    before, after, _args = _valid_pair(tmp_path)
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "calibration_compare",
            "--before-db",
            str(before),
            "--before-since",
            "2026-01-01T00:00:00Z",
            "--before-until",
            "2026-01-02T00:00:00Z",
            "--after-db",
            str(after),
            "--after-since",
            "2026-02-01T00:00:00Z",
            "--after-until",
            "2026-02-02T00:00:00Z",
        ],
    )

    main()

    output = capsys.readouterr().out
    table, json_text = output.split("\nJSON:\n", 1)
    assert "overall Brier" in table
    assert json.loads(json_text)["cohort"]["shared_scoreable_market_ids"] == [1]
