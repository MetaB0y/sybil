"""Tests for the arena Prometheus metrics subsystem (SYB-211)."""

import asyncio

from live.metrics import ArenaMetrics
from live.news_feed import NewsFeed
from sybil_client.types import Market


def _market(mid: int, name: str) -> Market:
    return Market(
        id=mid,
        name=name,
        yes_price_nanos=500_000_000,
        no_price_nanos=500_000_000,
        status="active",
    )


def _value(metrics: ArenaMetrics, name: str, labels: dict | None = None):
    return metrics.registry.get_sample_value(name, labels)


def test_registry_exposes_expected_series():
    metrics = ArenaMetrics()
    # Gauges start at 0; counters start at 0.0 — both are present (not None).
    for name in (
        "sybil_arena_selected_markets",
        "sybil_arena_selected_reference_markets",
        "sybil_news_feed_poll_in_progress",
        "sybil_news_feed_last_candidates",
        "sybil_news_feed_last_relevant_articles",
        "sybil_news_feed_latest_poll_start_timestamp_seconds",
        "sybil_news_feed_latest_poll_success_timestamp_seconds",
        "sybil_news_feed_polls_total",
        "sybil_news_feed_poll_errors_total",
        "sybil_news_feed_relevant_articles_total",
    ):
        assert _value(metrics, name) == 0, name


def test_two_instances_do_not_collide():
    # Each ArenaMetrics owns its own registry, so constructing several never
    # raises "Duplicated timeseries in CollectorRegistry".
    a = ArenaMetrics()
    b = ArenaMetrics()
    a.set_market_selection(5, 2)
    assert _value(a, "sybil_arena_selected_markets") == 5
    assert _value(b, "sybil_arena_selected_markets") == 0


def test_set_market_selection_sets_gauges():
    metrics = ArenaMetrics()
    metrics.set_market_selection(12, 7)
    assert _value(metrics, "sybil_arena_selected_markets") == 12
    assert _value(metrics, "sybil_arena_selected_reference_markets") == 7


def test_news_poll_success_updates_counters_and_gauges():
    metrics = ArenaMetrics()
    metrics.record_news_poll_start()
    assert _value(metrics, "sybil_news_feed_polls_total") == 1
    assert _value(metrics, "sybil_news_feed_poll_in_progress") == 1
    assert _value(metrics, "sybil_news_feed_latest_poll_start_timestamp_seconds") > 0

    metrics.record_news_poll_success(candidates=9, relevant_articles=3)
    assert _value(metrics, "sybil_news_feed_poll_in_progress") == 0
    assert _value(metrics, "sybil_news_feed_last_candidates") == 9
    assert _value(metrics, "sybil_news_feed_last_relevant_articles") == 3
    assert _value(metrics, "sybil_news_feed_relevant_articles_total") == 3
    assert _value(metrics, "sybil_news_feed_latest_poll_success_timestamp_seconds") > 0


def test_news_poll_error_increments_error_counter():
    metrics = ArenaMetrics()
    metrics.record_news_poll_start()
    metrics.record_news_poll_error()
    assert _value(metrics, "sybil_news_feed_poll_errors_total") == 1
    assert _value(metrics, "sybil_news_feed_poll_in_progress") == 0


def test_record_llm_call_counts_per_trader():
    metrics = ArenaMetrics()
    metrics.record_llm_call("Contrarian (Kelly)")
    metrics.record_llm_call("Contrarian (Kelly)")
    metrics.record_llm_call("Contrarian (Flat)")
    assert _value(metrics, "sybil_arena_llm_calls_total", {"trader": "Contrarian (Kelly)"}) == 2
    assert _value(metrics, "sybil_arena_llm_calls_total", {"trader": "Contrarian (Flat)"}) == 1


async def test_news_feed_run_drives_poll_metrics(monkeypatch):
    """A live poll cycle increments the poll counter and records its counts."""
    metrics = ArenaMetrics()
    feed = NewsFeed([_market(1, "X market")], api_key=None, poll_interval_s=30, metrics=metrics)

    async def fake_poll(http):
        return (3, 7)  # (delivered, candidates)

    monkeypatch.setattr(feed, "_poll_once", fake_poll)

    task = asyncio.create_task(feed.run())
    try:
        for _ in range(200):
            await asyncio.sleep(0.005)
            if _value(metrics, "sybil_news_feed_polls_total"):
                break
    finally:
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            pass

    assert _value(metrics, "sybil_news_feed_polls_total") == 1
    assert _value(metrics, "sybil_news_feed_last_candidates") == 7
    assert _value(metrics, "sybil_news_feed_last_relevant_articles") == 3
    assert _value(metrics, "sybil_news_feed_relevant_articles_total") == 3
