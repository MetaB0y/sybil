"""Tests for the live NewsFeed ingestion pipeline."""

from datetime import UTC, datetime
from unittest.mock import MagicMock

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


async def test_multi_market_article_fans_out_to_all_markets(monkeypatch):
    # AR-7: an article that matches several markets' feeds must be delivered to
    # ALL of them (not just the first polled), and matched_market_ids must
    # carry the full match set rather than a singleton.
    m1 = _market(1, "US and Iran sign a peace deal")
    m2 = _market(2, "Iran nuclear agreement reached")
    feed = NewsFeed([m1, m2], api_key=None, poll_interval_s=1)
    feed._warmed_up = True  # skip warm-start suppression

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

    delivered = await feed._poll_once(MagicMock())

    a1 = await feed.drain(1)
    a2 = await feed.drain(2)

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

    assert await feed._poll_once(MagicMock()) == 1
    assert await feed._poll_once(MagicMock()) == 0  # already seen
