"""Tests for the live NewsFeed ingestion pipeline."""

import asyncio
import logging
from datetime import UTC, datetime
from unittest.mock import AsyncMock, MagicMock

import pytest

from live.news_feed import (
    GATE_MODEL,
    LiveArticle,
    NewsFeed,
    PairedNewsBatchBarrier,
    ReferencePrices,
    build_search_query,
    llm_gate_batch,
)
from live.metrics import ArenaMetrics
from sybil_client.types import Market


def _market(mid: int, name: str) -> Market:
    return Market(
        id=mid,
        name=name,
        yes_price_nanos=500_000_000,
        no_price_nanos=500_000_000,
        status="active",
    )


def test_reference_price_cache_honors_exact_api_expiry_and_replacement():
    now_ms = [1_000]
    cache = ReferencePrices(clock_ms=lambda: now_ms[0])
    market = _market(1, "Market")
    market.reference_price_nanos = 400_000_000
    market.reference_price_expires_at_ms = 1_100
    cache.replace([market], {1})

    now_ms[0] = 1_100
    assert cache.get_price(1) == 0.4
    now_ms[0] = 1_101
    assert cache.get_price(1) is None

    cache.replace([], {1})
    assert cache.get_price(1) is None


def test_relevance_gate_uses_live_deepseek_model():
    assert GATE_MODEL == "deepseek/deepseek-v4-flash"


async def test_relevance_gate_disables_optional_reasoning():
    client = MagicMock()
    response = MagicMock()
    response.choices[0].message.content = "1"
    client.chat.completions.create = AsyncMock(return_value=response)

    assert await llm_gate_batch(client, ["one", "two"], "market") == [True, False]
    assert client.chat.completions.create.await_args.kwargs["extra_body"] == {
        "reasoning": {"effort": "none"}
    }


def test_search_query_retains_late_subject_terms():
    market = _market(
        46,
        "Will a Chinese company have one of the top 10 AI models by December 31?",
    )
    assert build_search_query(market) == (
        "Chinese company have one top 10 AI models December 31"
    )


async def test_relevance_gate_surfaces_provider_error_to_feed_policy():
    client = MagicMock()
    client.chat.completions.create = AsyncMock(side_effect=TimeoutError("provider"))

    with pytest.raises(TimeoutError):
        await llm_gate_batch(client, ["one", "two"], "market")


@pytest.mark.parametrize("answer", ["", "NONE because irrelevant", "0", "3", "1 and 2"])
async def test_relevance_gate_rejects_malformed_lossy_answers(answer):
    client = MagicMock()
    response = MagicMock()
    response.choices[0].message.content = answer
    client.chat.completions.create = AsyncMock(return_value=response)

    with pytest.raises(ValueError, match="relevance-gate"):
        await llm_gate_batch(client, ["one", "two"], "market")


async def test_multi_market_article_fans_out_to_all_markets(monkeypatch):
    # AR-7: an article that matches several markets' feeds must be delivered to
    # ALL of them (not just the first polled), and matched_market_ids must
    # carry the full match set rather than a singleton.
    m1 = _market(1, "US and Iran sign a peace deal")
    m2 = _market(2, "Iran nuclear agreement reached")
    feed = NewsFeed([m1, m2], api_key=None, poll_interval_s=1)
    feed._warmed_up = True  # skip warm-start suppression
    sub = feed.subscribe()

    shared = {
        "url": "http://example.com/shared",
        "title": "US and Iran reach a landmark agreement",
        "source": "wire",
        "published": datetime(2026, 7, 1, tzinfo=UTC),
    }

    async def fake_fetch_feed(http, url):
        # Every market feed surfaces the same article.
        return [dict(shared)]

    async def fake_text(http, url):
        return "full article text"

    monkeypatch.setattr(feed, "_fetch_feed", fake_fetch_feed)
    monkeypatch.setattr("live.news_feed.fetch_article_text", fake_text)

    delivered, _candidates = await feed._poll_once(MagicMock())

    a1 = await sub.drain(1)
    a2 = await sub.drain(2)

    assert len(a1) == 1
    assert len(a2) == 1
    assert set(a1[0].matched_market_ids) == {1, 2}
    assert set(a2[0].matched_market_ids) == {1, 2}
    assert delivered == 2  # delivered to both markets
    # The all-articles log is de-duped by url even though it fanned out.
    assert len(feed.drain_all_new()) == 1


async def test_seen_url_not_reprocessed_across_polls(monkeypatch):
    # Cross-poll dedup still holds: the same url is not re-delivered next poll.
    m1 = _market(1, "US and Iran sign a peace deal")
    feed = NewsFeed([m1], api_key=None, poll_interval_s=1)
    feed._warmed_up = True

    entry = {
        "url": "http://example.com/a",
        "title": "Peace talks advance",
        "source": "wire",
        "published": datetime(2026, 7, 1, tzinfo=UTC),
    }

    async def fake_fetch_feed(http, url):
        return [dict(entry)]

    async def fake_text(http, url):
        return "text"

    monkeypatch.setattr(feed, "_fetch_feed", fake_fetch_feed)
    monkeypatch.setattr("live.news_feed.fetch_article_text", fake_text)

    assert (await feed._poll_once(MagicMock()))[0] == 1
    assert (await feed._poll_once(MagicMock()))[0] == 0  # already seen


def _article(url: str, title: str = "headline") -> LiveArticle:
    return LiveArticle(
        url=url,
        title=title,
        source="wire",
        published=datetime(2026, 7, 1, tzinfo=UTC),
        full_text="body",
    )


async def _poll_delivering_one_article(feed, url, title):
    """Drive a single poll that delivers one article across every market feed."""
    feed._warmed_up = True
    entry = {
        "url": url,
        "title": title,
        "source": "wire",
        "published": datetime(2026, 7, 1, tzinfo=UTC),
    }

    async def fake_fetch_feed(http, _url):
        return [dict(entry)]

    async def fake_text(http, _url):
        return "text"

    _mp = pytest.MonkeyPatch()
    _mp.setattr(feed, "_fetch_feed", fake_fetch_feed)
    _mp.setattr("live.news_feed.fetch_article_text", fake_text)
    try:
        return await feed._poll_once(MagicMock())
    finally:
        _mp.undo()


async def test_two_subscribers_both_receive_same_article():
    # SYB-192 core regression: with the old shared-pending drain each article
    # reached exactly one trader, invalidating the Kelly-vs-Flat A/B. Now two
    # subscribers draining the same feed must BOTH see the same article.
    m1 = _market(1, "US and Iran sign a peace deal")
    feed = NewsFeed([m1], api_key=None, poll_interval_s=1)
    kelly = feed.subscribe(name="kelly")
    flat = feed.subscribe(name="flat")

    await _poll_delivering_one_article(feed, "http://ex/a", "Peace talks advance")

    a_kelly = await kelly.drain(1)
    a_flat = await flat.drain(1)

    assert len(a_kelly) == 1
    assert len(a_flat) == 1
    assert a_kelly[0].url == a_flat[0].url == "http://ex/a"
    # Draining one subscriber must NOT consume the other's copy.
    assert await kelly.drain(1) == []  # kelly already drained
    assert len(a_flat) == 1  # flat still had its own copy


async def test_paired_batch_barrier_holds_next_batch_until_both_arms_drain():
    feed = NewsFeed([_market(1, "Market")], api_key=None)
    upstream = feed.subscribe(name="paired")
    barrier = PairedNewsBatchBarrier(upstream, ("control", "stage1"), lambda _market_id: 0.5)
    control = barrier.view("control")
    stage1 = barrier.view("stage1")
    first = _article("http://ex/first")
    second = _article("http://ex/second")

    async with feed._lock:
        upstream._deliver(1, first)
    control_first = await control.drain(1)
    async with feed._lock:
        upstream._deliver(1, second)

    # The faster arm cannot consume or re-batch pending evidence while its pair
    # has not consumed the active batch.
    assert await control.drain(1) == []
    stage1_first = await stage1.drain(1)
    assert control_first is stage1_first
    assert control_first == [first]

    control_second = await control.drain(1)
    stage1_second = await stage1.drain(1)
    assert control_second is stage1_second
    assert control_second == [second]


async def test_paired_batch_can_be_retried_without_advancing_other_arm():
    feed = NewsFeed([_market(1, "Market")], api_key=None)
    upstream = feed.subscribe(name="paired")
    barrier = PairedNewsBatchBarrier(upstream, ("control", "stage1"), lambda _market_id: 0.5)
    control = barrier.view("control")
    stage1 = barrier.view("stage1")
    article = _article("http://ex/retry")

    async with feed._lock:
        upstream._deliver(1, article)

    first_attempt = await control.drain_batch(1)
    await control.retry_batch(1)
    retry = await control.drain_batch(1)
    assert retry is first_attempt
    await control.ack_batch(1)

    paired = await stage1.drain_batch(1)
    assert paired is first_attempt
    await stage1.ack_batch(1)


async def test_paired_batch_barrier_concurrent_drains_share_one_snapshot():
    feed = NewsFeed([_market(1, "Market")], api_key=None)
    upstream = feed.subscribe(name="paired")
    barrier = PairedNewsBatchBarrier(upstream, ("control", "stage1"), lambda _market_id: 0.5)
    article = _article("http://ex/shared")
    async with feed._lock:
        upstream._deliver(1, article)

    control_batch, stage1_batch = await asyncio.gather(
        barrier.view("control").drain(1),
        barrier.view("stage1").drain(1),
    )

    assert control_batch is stage1_batch
    assert control_batch == [article]


async def test_paired_batch_waits_without_consuming_articles_when_reference_expires():
    feed = NewsFeed([_market(1, "Market")], api_key=None)
    upstream = feed.subscribe(name="paired")
    current_reference = None
    barrier = PairedNewsBatchBarrier(
        upstream,
        ("control", "stage1"),
        lambda _market_id: current_reference,
    )
    article = _article("http://ex/waiting")
    async with feed._lock:
        upstream._deliver(1, article)

    control = barrier.view("control")
    assert await control.drain(1) == []
    current_reference = 0.6
    assert await control.drain(1) == [article]


async def test_unsubscribe_then_resubscribe_is_sane():
    # An unsubscribed view stops receiving; a fresh subscription only sees
    # articles delivered after it registered.
    m1 = _market(1, "Election market")
    feed = NewsFeed([m1], api_key=None, poll_interval_s=1)

    sub1 = feed.subscribe(name="s1")
    await _poll_delivering_one_article(feed, "http://ex/1", "first")
    assert len(await sub1.drain(1)) == 1

    feed.unsubscribe(sub1)
    # Unsubscribe is idempotent.
    feed.unsubscribe(sub1)

    await _poll_delivering_one_article(feed, "http://ex/2", "second")
    # sub1 no longer receives deliveries.
    assert await sub1.drain(1) == []

    # A fresh subscription only sees articles delivered after it joined.
    sub2 = feed.subscribe(name="s2")
    await _poll_delivering_one_article(feed, "http://ex/3", "third")
    got = await sub2.drain(1)
    assert len(got) == 1
    assert got[0].url == "http://ex/3"


async def test_subscriber_queue_bounds_drop_oldest(caplog):
    # A stalled subscriber's queue is bounded drop-oldest, with a warning.
    m1 = _market(1, "Bounded market")
    feed = NewsFeed([m1], api_key=None, poll_interval_s=1)
    sub = feed.subscribe(max_queue=2, name="stalled")

    async with feed._lock:
        sub._deliver(1, _article("http://ex/1", "first"))
        sub._deliver(1, _article("http://ex/2", "second"))
        with caplog.at_level(logging.WARNING, logger="live.news_feed"):
            sub._deliver(1, _article("http://ex/3", "third"))

    drained = await sub.drain(1)
    # Oldest dropped; only the two most-recent survive, in order.
    assert [a.url for a in drained] == ["http://ex/2", "http://ex/3"]
    assert "dropping oldest" in caplog.text


async def test_gate_credit_failure_is_visible_and_backed_off():
    class CreditError(Exception):
        status_code = 402

    metrics = ArenaMetrics()
    feed = NewsFeed(
        [_market(1, "Market")],
        api_key="test",
        poll_interval_s=1,
        metrics=metrics,
    )
    feed._llm_client.chat.completions.create = AsyncMock(side_effect=CreditError("credit"))

    assert await _poll_delivering_one_article(feed, "http://ex/credit-1", "first") == (1, 1)
    assert await _poll_delivering_one_article(feed, "http://ex/credit-2", "second") == (1, 1)

    # The first failure is classified; the second poll passes evidence through
    # without hammering a known non-retryable provider state.
    assert feed._llm_client.chat.completions.create.await_count == 1
    assert metrics.registry.get_sample_value(
        "sybil_arena_llm_provider_failures_total",
        {"component": "news-gate", "kind": "credit"},
    ) == 1
    assert metrics.registry.get_sample_value(
        "sybil_arena_llm_provider_degraded",
        {"component": "news-gate"},
    ) == 1
