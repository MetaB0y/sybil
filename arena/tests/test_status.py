"""Status rendering helpers."""

import sqlite3

import pytest

from live import queries
from live.status import _parse_order_suppressions, _parse_provider_health


def test_provider_health_metrics_are_grouped_by_component():
    states = _parse_provider_health(
        """
# TYPE sybil_arena_llm_provider_degraded gauge
sybil_arena_llm_provider_degraded{component="news-gate"} 1
# TYPE sybil_arena_llm_provider_last_success_timestamp_seconds gauge
sybil_arena_llm_provider_last_success_timestamp_seconds{component="news-gate"} 12
# TYPE sybil_arena_llm_provider_backoff_until_timestamp_seconds gauge
sybil_arena_llm_provider_backoff_until_timestamp_seconds{component="news-gate"} 90
# TYPE sybil_arena_llm_provider_failures_total counter
sybil_arena_llm_provider_failures_total{component="news-gate",kind="credit"} 3
"""
    )

    assert states == {
        "news-gate": {
            "degraded": True,
            "last_success": 12.0,
            "backoff_until": 90.0,
            "failures": {"credit": 3.0},
        }
    }


def test_order_suppression_metrics_are_grouped_by_reason():
    suppressions = _parse_order_suppressions(
        """
# TYPE sybil_arena_orders_suppressed_total counter
sybil_arena_orders_suppressed_total{trader="Contrarian (Flat)",reason="below_min_notional"} 4
sybil_arena_orders_suppressed_total{trader="Noise-1",reason="below_min_notional"} 2
"""
    )

    assert suppressions == {"below_min_notional": 6.0}


def test_llm_cost_uses_requested_window_and_recorded_cost():
    conn = sqlite3.connect(":memory:")
    conn.execute(
        """
        CREATE TABLE token_usage (
            trader_name TEXT,
            timestamp TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            duration_s REAL,
            usd_cost REAL,
            cost_source TEXT
        )
        """
    )
    conn.executemany(
        "INSERT INTO token_usage VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            ("Analyst", "2026-07-21T00:00:00+00:00", 10_000, 1_000, 99.0, 9.0, "price_table"),
            ("Analyst", "2026-07-22T00:30:00+00:00", 100, 40, 2.5, 0.0012, "response"),
            ("Analyst", "2026-07-22T00:45:00+00:00", 200, 60, 3.5, 0.0024, "response"),
        ],
    )

    result = queries.get_llm_cost(conn, cutoff="2026-07-22T00:24:18.476000+00:00")

    assert result is not None
    assert result.loc[0, "calls"] == 2
    assert result.loc[0, "prompt_tokens"] == 300
    assert result.loc[0, "completion_tokens"] == 100
    assert result.loc[0, "max_completion_tokens"] == 60
    assert result.loc[0, "avg_latency_s"] == pytest.approx(3.0)
    assert result.loc[0, "max_latency_s"] == pytest.approx(3.5)
    assert result.loc[0, "recorded_cost_usd"] == pytest.approx(0.0036)
    assert result.loc[0, "cost_sources"] == "response"
