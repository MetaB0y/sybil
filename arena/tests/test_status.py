"""Status rendering helpers."""

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
