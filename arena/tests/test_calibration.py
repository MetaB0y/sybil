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
            countercase TEXT
        );
        INSERT INTO decisions
            (trader_name, market_id, market_name, timestamp, fair_value,
             market_price, orders, raw_fair_value, effective_fair_value,
             fair_value_age_s, confidence, countercase)
        VALUES
            ('Alice (Kelly)', 1, 'M1', '2026-01-01T00:00:00Z', 0.75,
             0.70, '[{"side":"BUY_YES"}]', 0.80, 0.75, 30, 0.60, 'c'),
            ('Alice (Flat)', 2, 'M2', '2026-01-01T00:01:00Z', 0.45,
             0.30, '[]', 0.40, 0.45, 60, 0.50, 'c'),
            ('Bob (Kelly)', 1, 'M1', '2026-01-01T00:02:00Z', 0.20,
             0.70, '[]', 0.20, 0.20, 90, 0.40, 'c');

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
        "mean_pnl": -1.0,
        "median_pnl": -1.0,
        "min_pnl": -3.0,
        "max_pnl": 1.0,
    }

    report = format_report(result)
    assert "Calibration by persona" in report
    assert "Market-price baseline Brier" in report
    assert "NativeNoiseTrader PnL baseline" in report
