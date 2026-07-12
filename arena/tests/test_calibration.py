import sqlite3
from datetime import datetime, timedelta, timezone
from hashlib import sha256
import json

import pytest

from live.personas import PERSONAS
from scripts.calibration import _parse_market_ids, analyze_decisions_db, format_report


def _fixture_db(path):
    conn = sqlite3.connect(path)
    conn.executescript("""
        CREATE TABLE market_outcomes (
            market_id INTEGER PRIMARY KEY,
            outcome REAL NOT NULL
        );
        INSERT INTO market_outcomes (market_id, outcome) VALUES
            (1, 1.0),
            (2, 0.0);

        CREATE TABLE decisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            market_id INTEGER,
            market_name TEXT,
            timestamp TEXT,
            fair_value REAL,
            market_price REAL,
            orders TEXT,
            raw_fair_value REAL,
            effective_fair_value REAL,
            fair_value_age_s REAL,
            confidence REAL,
            countercase TEXT,
            rejection_reason TEXT,
            market_category TEXT,
            market_tags TEXT,
            analysis_batch_id TEXT,
            analysis_reference_price REAL
        );
        INSERT INTO decisions
            (trader_name, market_id, market_name, timestamp, fair_value,
             market_price, orders, raw_fair_value, effective_fair_value,
             fair_value_age_s, confidence, countercase, rejection_reason,
             market_category, market_tags)
        VALUES
            ('Alice (Kelly)', 1, 'M1', '2026-01-01T00:00:00Z', 0.75,
             0.70, '[{"side":"BUY_YES"}]', 0.80, 0.75, 30, 0.60, 'c',
             NULL, 'Politics', '["elections"]'),
            ('Alice (Flat)', 2, 'M2', '2026-01-01T00:01:00Z', 0.45,
             0.30, '[]', 0.40, 0.45, 60, 0.50, 'c',
             'below_min_edge', 'Science', '["space"]'),
            ('Bob (Kelly)', 1, 'M1', '2026-01-01T00:02:00Z', 0.20,
             0.70, '[]', 0.20, 0.20, 90, 0.40, 'c',
             'hold_position', 'Politics', '["elections"]');

        CREATE TABLE portfolio_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            timestamp TEXT,
            balance REAL,
            portfolio_value REAL,
            pnl REAL,
            positions TEXT
        );
        INSERT INTO portfolio_snapshots
            (trader_name, timestamp, balance, portfolio_value, pnl, positions)
        VALUES
            ('Noise-0', '2026-01-01T00:00:00Z', 50, 51, 1, '{}'),
            ('Noise-1', '2026-01-01T00:00:00Z', 50, 47, -3, '{}'),
            ('Alice (Kelly)', '2026-01-01T00:00:00Z', 500, 505, 5, '{}');
    """)
    conn.commit()
    conn.close()


def _add_stage1_experiment(conn, experiment_id="exp-strict"):
    display_name = PERSONAS["news_trader"]["name"]
    configuration = {
        "market_ids": [1, 2],
        "personas": ["news_trader"],
        "persona_display_name_sha256": {"news_trader": sha256(display_name.encode()).hexdigest()},
        "variants": [{"id": "control"}, {"id": "stage1"}],
    }
    conn.executescript("""
        CREATE TABLE live_experiments (
            experiment_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL,
            started_at_utc TEXT NOT NULL,
            configuration_json TEXT NOT NULL
        );
        CREATE TABLE token_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trader_name TEXT,
            timestamp TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            model TEXT,
            duration_s REAL,
            usd_cost REAL,
            cost_source TEXT
        );
    """)
    conn.execute(
        "INSERT INTO live_experiments VALUES (?, ?, ?, ?)",
        (
            experiment_id,
            "syb-114-stage1-ab",
            "2026-01-01T00:00:00Z",
            json.dumps(configuration),
        ),
    )
    return display_name


def _add_covered_stage1_snapshots(
    conn,
    display_name,
    *,
    experiment_id="exp-strict",
    duration_minutes=24 * 60,
):
    start = datetime(2026, 1, 1, tzinfo=timezone.utc)
    names = [
        f"{display_name} [SYB-114:{experiment_id}:{variant}] (Flat)"
        for variant in ("control", "stage1")
    ]
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, ?, 0, 0, 0, '{}')""",
        [
            (trader_name, (start + timedelta(minutes=minute)).isoformat())
            for trader_name in names
            for minute in range(0, duration_minutes, 5)
        ],
    )


def test_calibration_harness_computes_brier_reliability_and_noise_baseline(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)

    result = analyze_decisions_db(str(db_path), bins=2)
    alice = next(persona for persona in result["personas"] if persona["persona"] == "Alice")
    bob = next(persona for persona in result["personas"] if persona["persona"] == "Bob")
    noise = result["baselines"]["native_noise_trader_pnl"]

    assert result["outcomes"] == {
        "source": "explicit",
        "count": 2,
        "used_decision_rows": 3,
        "raw_scoreable_decision_rows": 3,
        "duplicate_batch_decision_rows_excluded": 0,
    }
    assert alice["n"] == 2
    assert alice["brier"] == 0.1325
    assert alice["raw_brier"] == 0.10
    assert alice["market_price_brier"] == pytest.approx(0.09)
    assert alice["rejection_calibration"]["acted_n"] == 1
    assert alice["rejection_calibration"]["rejected_n"] == 1
    assert alice["reliability"][0]["n"] == 1
    assert alice["reliability"][0]["empirical_yes_rate"] == 0.0
    assert alice["reliability"][1]["n"] == 1
    assert alice["reliability"][1]["empirical_yes_rate"] == 1.0
    assert bob["brier"] == 0.6400000000000001
    assert noise == {
        "n": 2,
        "mode": "cumulative",
        "mean_pnl": -1.0,
        "median_pnl": -1.0,
        "min_pnl": -3.0,
        "max_pnl": 1.0,
    }

    report = format_report(result)
    assert "Full-arm calibration by persona" in report
    assert "Market-price baseline Brier" in report
    assert "NativeNoiseTrader PnL baseline" in report


def test_calibration_pnl_uses_window_deltas_for_all_live_arms(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, ?, 0, 0, ?, '{}')""",
        [
            ("Alice (Flat)", "2026-01-01T00:00:00Z", 10.0),
            ("Alice (Flat)", "2026-01-01T00:01:30Z", 14.0),
            ("Alice (Flat)", "2026-01-01T00:03:00Z", 30.0),
            ("Alice (Kelly)", "2026-01-01T00:01:30Z", 8.0),
            ("Noise-0", "2026-01-01T00:01:30Z", 4.0),
            ("Noise-0", "2026-01-01T00:03:00Z", 20.0),
        ],
    )
    conn.commit()
    conn.close()

    result = analyze_decisions_db(
        str(db_path),
        since="2026-01-01T00:01:00Z",
        until="2026-01-01T00:02:00Z",
    )

    assert result["portfolio_pnl"] == {
        "flat": {
            "n": 1,
            "mode": "window_delta",
            "mean_pnl": 4.0,
            "median_pnl": 4.0,
            "min_pnl": 4.0,
            "max_pnl": 4.0,
        },
        "kelly": {
            "n": 1,
            "mode": "window_delta",
            "mean_pnl": 3.0,
            "median_pnl": 3.0,
            "min_pnl": 3.0,
            "max_pnl": 3.0,
        },
        "native_noise": {
            "n": 1,
            "mode": "window_delta",
            "mean_pnl": 3.0,
            "median_pnl": 3.0,
            "min_pnl": 3.0,
            "max_pnl": 3.0,
        },
    }
    assert result["portfolio_pnl_by_trader"] == {
        "flat": {"Alice (Flat)": 4.0},
        "kelly": {"Alice (Kelly)": 3.0},
        "native_noise": {"Noise-0": 3.0},
    }
    report = format_report(result)
    assert "Flat-arm PnL: n=1 mode=window_delta mean=4.00" in report
    assert "Kelly-arm PnL: n=1 mode=window_delta mean=3.00" in report
    assert "Flat-arm PnL by durable trader identity" in report
    assert "Alice (Flat): 4.00" in report


def test_window_pnl_includes_movement_after_in_window_startup_baseline(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES ('Fresh Experiment (Flat)', ?, 0, 0, ?, '{}')""",
        [
            ("2026-01-01T00:01:00Z", 0.0),
            ("2026-01-01T00:02:00Z", 6.5),
        ],
    )
    conn.commit()
    conn.close()

    result = analyze_decisions_db(
        str(db_path),
        since="2026-01-01T00:01:00Z",
        until="2026-01-01T00:03:00Z",
    )

    assert result["portfolio_pnl_by_trader"]["flat"] == {"Fresh Experiment (Flat)": 6.5}


def test_calibration_keeps_stage1_ab_flat_variants_separate(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, '2026-01-01T00:03:00Z', 0, 0, ?, '{}')""",
        [
            ("News Trader [SYB-114:exp:control] (Flat)", 3.0),
            ("News Trader [SYB-114:exp:stage1] (Flat)", 7.0),
        ],
    )
    conn.executemany(
        """INSERT INTO decisions
           (trader_name, market_id, market_name, timestamp, fair_value,
            market_price, orders, raw_fair_value, effective_fair_value,
            fair_value_age_s, confidence, countercase, rejection_reason,
            market_category, market_tags, analysis_batch_id, analysis_reference_price)
           VALUES (?, 1, 'M1', '2026-01-01T00:03:00Z', ?, ?,
                   '[{"side":"BUY_YES"}]', ?, ?, 0, 0.7, 'c', NULL,
                   'Politics', '[]', ?, ?)""",
        [
            (
                "News Trader [SYB-114:exp:control] (Flat)",
                0.60,
                0.40,
                0.60,
                0.60,
                "batch-shared",
                0.52,
            ),
            (
                "News Trader [SYB-114:exp:control] (Flat)",
                0.61,
                0.45,
                0.61,
                0.61,
                "batch-shared",
                0.52,
            ),
            (
                "News Trader [SYB-114:exp:stage1] (Flat)",
                0.70,
                0.80,
                0.70,
                0.70,
                "batch-shared",
                0.52,
            ),
            (
                "News Trader [SYB-114:exp:stage1] (Flat)",
                0.75,
                0.85,
                0.75,
                0.75,
                "batch-stage1-only",
                0.53,
            ),
        ],
    )
    conn.commit()
    conn.close()

    result = analyze_decisions_db(str(db_path))

    assert result["portfolio_pnl_by_trader"]["flat"] == {
        "News Trader [SYB-114:exp:control] (Flat)": 3.0,
        "News Trader [SYB-114:exp:stage1] (Flat)": 7.0,
    }
    assert [
        persona["persona"] for persona in result["personas"] if "SYB-114" in persona["persona"]
    ] == [
        "News Trader [SYB-114:exp:control]",
        "News Trader [SYB-114:exp:stage1]",
    ]
    report = format_report(result)
    assert "News Trader [SYB-114:exp:control]" in report
    assert "News Trader [SYB-114:exp:stage1]" in report
    assert "News Trader [SYB-114:exp:control] (Flat): 3.00" in report
    assert "News Trader [SYB-114:exp:stage1] (Flat): 7.00" in report
    assert result["analysis_batches"]["duplicate_decision_rows_excluded"] == 1
    assert result["analysis_batches"]["control_stage1_matching"] == [
        {
            "experiment_id": "exp",
            "persona": "News Trader",
            "comparison_semantics": "Stage1 minus control on exact analysis_batch_id intersection",
            "comparable": True,
            "not_comparable_reason": None,
            "matched_count": 1,
            "unmatched_control_count": 0,
            "unmatched_stage1_count": 1,
            "control": {
                "n": 1,
                "brier": pytest.approx(0.16),
                "market_price_brier": pytest.approx(0.2304),
                "analysis_market_prices": [0.52],
            },
            "stage1": {
                "n": 1,
                "brier": pytest.approx(0.09),
                "market_price_brier": pytest.approx(0.2304),
                "analysis_market_prices": [0.52],
            },
            "stage1_minus_control": {
                "brier": pytest.approx(-0.07),
                "market_price_brier": pytest.approx(0.0),
            },
        }
    ]
    assert "Stage 1 matched-batch experiment comparison (primary)" in report
    assert "Full-arm calibration by persona (diagnostic" in report
    assert "matched=1 unmatched-control=0 unmatched-stage1=1" in report


def test_stage1_ab_with_only_asymmetric_batches_is_not_comparable(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.executemany(
        """INSERT INTO decisions
           (trader_name, market_id, market_name, timestamp, fair_value,
            market_price, orders, analysis_batch_id, analysis_reference_price)
           VALUES (?, 1, 'M1', '2026-01-01T00:03:00Z', ?, ?, '[]', ?, 0.55)""",
        [
            ("News Trader [SYB-114:exp:control] (Flat)", 0.60, 0.40, "control-only"),
            ("News Trader [SYB-114:exp:stage1] (Flat)", 0.70, 0.80, "stage1-only"),
        ],
    )
    conn.commit()
    conn.close()

    result = analyze_decisions_db(str(db_path))
    comparison = result["analysis_batches"]["control_stage1_matching"][0]

    assert comparison["comparable"] is False
    assert comparison["not_comparable_reason"] == "no matched analysis batches"
    assert comparison["control"] == {
        "n": 0,
        "brier": None,
        "market_price_brier": None,
        "analysis_market_prices": [],
    }
    assert comparison["stage1_minus_control"] == {
        "brier": None,
        "market_price_brier": None,
    }
    report = format_report(result)
    assert "not comparable (no matched analysis batches)" in report


def test_calibration_window_reason_counterfactuals_categories_and_surprises(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)

    result = analyze_decisions_db(
        str(db_path),
        bins=2,
        since="2026-01-01T00:00:30Z",
        until="2026-01-01T00:02:00Z",
        top_n=3,
    )

    assert result["overall"]["n"] == 1
    assert result["window"] == {
        "since": "2026-01-01T00:00:30+00:00",
        "until": "2026-01-01T00:02:00+00:00",
        "semantics": "since inclusive, until exclusive",
    }
    rejection = result["personas"][0]["rejection_calibration"]
    assert rejection["by_reason"]["below_min_edge"] == {
        "n": 1,
        "would_have_profited_n": 0,
        "would_have_lost_or_broken_even_n": 1,
        "would_have_profited_rate": 0.0,
    }
    assert result["overall"]["by_category_brier"] == [
        {"category": "Science", "n": 1, "brier": 0.2025}
    ]
    assert result["surprises"] == []

    acted = analyze_decisions_db(str(db_path), until="2026-01-01T00:01:00Z", top_n=1)
    assert acted["surprises"][0]["market_id"] == 1
    assert acted["surprises"][0]["absolute_error"] == 0.25


def test_calibration_pins_and_reports_exact_market_cohort(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)

    result = analyze_decisions_db(str(db_path), market_ids={2})

    assert result["cohort"] == {
        "requested_market_ids": [2],
        "scored_market_ids": [2],
    }
    assert result["overall"]["n"] == 1
    assert [persona["persona"] for persona in result["personas"]] == ["Alice"]
    assert result["surprises"] == []
    assert result["portfolio_pnl_scope"] == "all_trader_positions"
    report = format_report(result)
    assert "Pinned forecast cohort: 2" in report
    assert "Portfolio PnL scope: all trader positions" in report


def test_persisted_stage1_report_locks_scope_and_keeps_pnl_without_outcomes(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    display_name = _add_stage1_experiment(conn)
    conn.execute("DELETE FROM market_outcomes")
    conn.execute("DELETE FROM decisions")
    conn.execute("DELETE FROM portfolio_snapshots")
    control_flat = f"{display_name} [SYB-114:exp-strict:control] (Flat)"
    stage1_flat = f"{display_name} [SYB-114:exp-strict:stage1] (Flat)"
    control_analyst = f"{display_name} [SYB-114:exp-strict:control] (Analyst)"
    stage1_analyst = f"{display_name} [SYB-114:exp-strict:stage1] (Analyst)"
    conn.executemany(
        """INSERT INTO decisions
           (trader_name, market_id, market_name, timestamp, fair_value,
            market_price, orders, analysis_batch_id, analysis_reference_price)
           VALUES (?, 1, 'M1', ?, ?, ?, '[]', ?, 0.45)""",
        [
            (control_flat, "2026-01-01T01:00:00Z", 0.55, 0.40, "shared"),
            (control_flat, "2026-01-01T01:05:00Z", 0.56, 0.41, "shared"),
            (stage1_flat, "2026-01-01T01:00:00Z", 0.60, 0.40, "shared"),
            (
                "News Trader [SYB-114:other:control] (Flat)",
                "2026-01-01T01:00:00Z",
                0.99,
                0.99,
                "foreign",
            ),
        ],
    )
    start = datetime(2026, 1, 1, tzinfo=timezone.utc)
    snapshot_rows = []
    for trader_name, final_pnl in ((control_flat, 3.0), (stage1_flat, 6.0)):
        snapshot_rows.extend(
            (
                trader_name,
                (start + timedelta(minutes=minute)).isoformat(),
                final_pnl * minute / 1555,
            )
            for minute in range(0, 1560, 5)
        )
    snapshot_rows.append(
        (
            "News Trader [SYB-114:other:control] (Flat)",
            "2026-01-02T01:00:00Z",
            999.0,
        )
    )
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, ?, 0, 0, ?, '{}')""",
        snapshot_rows,
    )
    conn.executemany(
        """INSERT INTO token_usage
           (trader_name, timestamp, prompt_tokens, completion_tokens, model,
            duration_s, usd_cost, cost_source)
           VALUES (?, ?, ?, ?, 'model', 1, ?, 'response')""",
        [
            (control_analyst, "2026-01-01T01:00:00Z", 100, 10, 0.10),
            (control_analyst, "2026-01-01T02:00:00Z", 200, 20, 0.20),
            (stage1_analyst, "2026-01-01T01:00:00Z", 120, 12, 0.15),
            ("News Trader [SYB-114:other:control] (Analyst)", "2026-01-01T01:00:00Z", 1, 1, 99.0),
        ],
    )
    conn.commit()
    conn.close()

    result = analyze_decisions_db(
        str(db_path),
        experiment_id="exp-strict",
        until="2026-01-02T02:00:00Z",
    )

    assert result["overall"]["n"] == 0
    assert result["outcomes"]["count"] == 0
    assert result["cohort"]["requested_market_ids"] == [1, 2]
    assert result["window"]["since"] == "2026-01-01T00:00:00+00:00"
    comparison = result["experiment"]["comparisons"][0]
    assert comparison["matched_analysis_batch_count"] == 1
    assert comparison["control"]["calls"] == 2
    assert comparison["control"]["usd"] == pytest.approx(0.30)
    assert comparison["control"]["decision_rows"] == 2
    assert comparison["control"]["analysis_batch_count"] == 1
    assert comparison["control"]["usd_per_decision"] == pytest.approx(0.15)
    assert comparison["stage1"]["calls"] == 1
    assert comparison["stage1"]["pnl"] == 6.0
    assert comparison["stage1_minus_control"]["pnl"] == 3.0
    coverage = result["experiment"]["snapshot_coverage"]
    assert coverage["coverage_complete"] is True
    assert coverage["expected_cadence_seconds"] == 300
    assert coverage["maximum_allowed_gap_seconds"] == 600
    assert {arm["snapshot_count"] for arm in coverage["arms"]} == {312}
    assert {arm["max_consecutive_gap_seconds"] for arm in coverage["arms"]} == {300.0}
    assert {arm["end_lag_seconds"] for arm in coverage["arms"]} == {300.0}
    assert result["experiment"]["flat_pnl_by_durable_identity"] == {
        control_flat: 3.0,
        stage1_flat: 6.0,
    }
    report = format_report(result)
    assert "strict >=24h window" in report
    assert "calls=2 usd=0.30000" in report
    assert "pnl=6.00" in report
    assert "Portfolio snapshot window coverage: complete" in report
    assert "No explicit outcomes: forecast metrics are unavailable" in report
    assert "999" not in report


def test_persisted_stage1_report_requires_24h_or_records_exploratory_override(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    _add_stage1_experiment(conn)
    conn.commit()
    conn.close()

    with pytest.raises(ValueError, match="at least 24 hours"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            until="2026-01-01T06:00:00Z",
        )

    result = analyze_decisions_db(
        str(db_path),
        experiment_id="exp-strict",
        until="2026-01-01T06:00:00Z",
        exploratory_short_window=True,
    )
    assert result["experiment"]["window"]["exploratory_short_window_override"] is True
    assert result["experiment"]["snapshot_coverage"]["coverage_complete"] is False
    assert "EXPLORATORY SHORT-WINDOW OVERRIDE" in format_report(result)
    assert "INCOMPLETE (exploratory report only)" in format_report(result)

    with pytest.raises(ValueError, match="derives --since"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            since="2026-01-01T00:00:00Z",
            until="2026-01-02T01:00:00Z",
        )
    with pytest.raises(ValueError, match="derives the frozen cohort"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            until="2026-01-02T01:00:00Z",
            market_ids={1, 2},
        )


def test_strict_stage1_report_rejects_requested_26h_with_only_early_snapshots(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    display_name = _add_stage1_experiment(conn)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, ?, 0, 0, 0, '{}')""",
        [
            (
                f"{display_name} [SYB-114:exp-strict:{variant}] (Flat)",
                timestamp,
            )
            for variant in ("control", "stage1")
            for timestamp in ("2026-01-01T00:00:00Z", "2026-01-01T00:05:00Z")
        ],
    )
    conn.commit()
    conn.close()

    with pytest.raises(ValueError, match="window coverage incomplete"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            until="2026-01-02T02:00:00Z",
        )

    exploratory = analyze_decisions_db(
        str(db_path),
        experiment_id="exp-strict",
        until="2026-01-02T02:00:00Z",
        exploratory_short_window=True,
    )
    coverage = exploratory["experiment"]["snapshot_coverage"]
    assert coverage["coverage_complete"] is False
    assert all(arm["latest_snapshot_utc"] is not None for arm in coverage["arms"])
    assert all(arm["end_lag_seconds"] == 93300.0 for arm in coverage["arms"])

    conn = sqlite3.connect(db_path)
    conn.executemany(
        """INSERT INTO portfolio_snapshots
           (trader_name, timestamp, balance, portfolio_value, pnl, positions)
           VALUES (?, '2026-01-02T01:55:00Z', 0, 0, 0, '{}')""",
        [
            (f"{display_name} [SYB-114:exp-strict:{variant}] (Flat)",)
            for variant in ("control", "stage1")
        ],
    )
    conn.commit()
    conn.close()
    with pytest.raises(ValueError, match="maximum consecutive snapshot gap"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            until="2026-01-02T02:00:00Z",
        )


def test_experiment_report_rejects_future_until_before_snapshot_tolerance(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    display_name = _add_stage1_experiment(conn)
    # At start+23h55, these snapshots satisfy every endpoint/gap tolerance
    # for a requested 24h interval. The future end must still fail first.
    _add_covered_stage1_snapshots(conn, display_name)
    conn.commit()
    conn.close()

    now = datetime(2026, 1, 1, 23, 55, tzinfo=timezone.utc)
    for exploratory in (False, True):
        with pytest.raises(ValueError, match="--until cannot be in the future"):
            analyze_decisions_db(
                str(db_path),
                experiment_id="exp-strict",
                until="2026-01-02T00:00:00Z",
                exploratory_short_window=exploratory,
                now=now,
            )


def test_strict_experiment_outcomes_are_filtered_to_frozen_cohort(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    display_name = _add_stage1_experiment(conn)
    _add_covered_stage1_snapshots(conn, display_name)
    conn.execute("DELETE FROM market_outcomes")
    conn.execute("INSERT INTO market_outcomes (market_id, outcome) VALUES (99, 1.0)")
    conn.commit()
    conn.close()

    report_args = {
        "experiment_id": "exp-strict",
        "until": "2026-01-02T00:00:00Z",
        "now": datetime(2026, 1, 2, 1, tzinfo=timezone.utc),
    }
    foreign_only = analyze_decisions_db(str(db_path), **report_args)
    assert foreign_only["outcomes"]["source"] == "explicit_unavailable"
    assert foreign_only["outcomes"]["count"] == 0
    assert "No explicit outcomes" in format_report(foreign_only)

    conn = sqlite3.connect(db_path)
    conn.execute("INSERT INTO market_outcomes (market_id, outcome) VALUES (1, 1.0)")
    conn.commit()
    conn.close()
    mixed = analyze_decisions_db(str(db_path), **report_args)
    assert mixed["outcomes"]["source"] == "explicit"
    assert mixed["outcomes"]["count"] == 1
    assert "No explicit outcomes" not in format_report(mixed)


def test_persisted_stage1_report_fails_closed_on_identity_fingerprint_drift(tmp_path):
    db_path = tmp_path / "decisions.db"
    _fixture_db(db_path)
    conn = sqlite3.connect(db_path)
    _add_stage1_experiment(conn)
    configuration = json.loads(
        conn.execute("SELECT configuration_json FROM live_experiments").fetchone()[0]
    )
    configuration["persona_display_name_sha256"]["news_trader"] = "0" * 64
    conn.execute(
        "UPDATE live_experiments SET configuration_json = ?",
        (json.dumps(configuration),),
    )
    conn.commit()
    conn.close()

    with pytest.raises(ValueError, match="display-name fingerprint drifted"):
        analyze_decisions_db(
            str(db_path),
            experiment_id="exp-strict",
            until="2026-01-02T01:00:00Z",
        )


def test_market_id_filter_parser_is_strict_and_deduplicates():
    assert _parse_market_ids(None) is None
    assert _parse_market_ids("") is None
    assert _parse_market_ids("2, 1,2") == frozenset({1, 2})
    with pytest.raises(ValueError, match="comma-separated integers"):
        _parse_market_ids("1,two")
    with pytest.raises(ValueError, match="non-negative"):
        _parse_market_ids("-1,2")
