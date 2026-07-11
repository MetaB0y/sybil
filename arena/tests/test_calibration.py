import sqlite3

import pytest

from scripts.calibration import analyze_decisions_db, format_report


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
            market_tags TEXT
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
    assert "Calibration by persona" in report
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
    report = format_report(result)
    assert "Flat-arm PnL: n=1 mode=window_delta mean=4.00" in report
    assert "Kelly-arm PnL: n=1 mode=window_delta mean=3.00" in report


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
