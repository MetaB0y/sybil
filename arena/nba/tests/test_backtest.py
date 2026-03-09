"""Tests for the backtest module."""

import asyncio
from datetime import datetime, timedelta

import pytest

from backtest import (
    Dataset,
    Event,
    FinalScore,
    MarketSpec,
    NewsItem,
    NewsScheduler,
    SimulatedClock,
    drain_queue,
)


class TestSimulatedClock:
    """Tests for SimulatedClock."""

    def test_initial_time(self):
        """Clock starts at sim_start."""
        start = datetime(2025, 1, 15, 19, 0, 0)
        clock = SimulatedClock(sim_start=start)
        # Before starting, now() returns sim_start
        assert clock.now() == start

    def test_time_compression(self):
        """Time compression works correctly."""
        clock = SimulatedClock(
            sim_start=datetime(2025, 1, 15, 19, 0, 0),
            compression_ratio=60.0,
        )
        # 1 real second = 60 simulated seconds
        assert clock.sim_to_real_seconds(60) == 1.0
        assert clock.real_to_sim_seconds(1) == 60.0

    def test_elapsed_before_start(self):
        """Elapsed time is zero before start."""
        clock = SimulatedClock(sim_start=datetime(2025, 1, 15, 19, 0, 0))
        assert clock.elapsed_real_time() == timedelta(0)

    def test_is_past(self):
        """is_past works correctly."""
        start = datetime(2025, 1, 15, 19, 0, 0)
        clock = SimulatedClock(sim_start=start)
        clock.start()
        # Past times should return True
        assert clock.is_past(start - timedelta(hours=1))
        # Start time should be past (or at)
        assert clock.is_past(start)


class TestNewsItem:
    """Tests for NewsItem."""

    def test_serialization_roundtrip(self):
        """NewsItem can be serialized and deserialized."""
        news = NewsItem(
            timestamp=datetime(2025, 1, 15, 19, 30, 0),
            headline="Test headline",
            content="Test content",
            source="lineup",
            event_id="test_event",
            metadata={"key": "value"},
        )
        data = news.to_dict()
        restored = NewsItem.from_dict(data)

        assert restored.timestamp == news.timestamp
        assert restored.headline == news.headline
        assert restored.content == news.content
        assert restored.source == news.source
        assert restored.event_id == news.event_id
        assert restored.metadata == news.metadata


class TestEvent:
    """Tests for Event."""

    def test_moneyline_market_name(self):
        """Moneyline market name is generated correctly."""
        event = Event(
            event_id="test",
            home_team="Boston Celtics",
            away_team="Los Angeles Lakers",
            commence_time=datetime(2025, 1, 15, 19, 0, 0),
            end_time=datetime(2025, 1, 15, 22, 0, 0),
            actual_outcome="home",
        )
        assert event.moneyline_market_name == "Boston Celtics beats Los Angeles Lakers"

    def test_serialization_roundtrip(self):
        """Event can be serialized and deserialized."""
        event = Event(
            event_id="test",
            home_team="Boston Celtics",
            away_team="Los Angeles Lakers",
            commence_time=datetime(2025, 1, 15, 19, 0, 0),
            end_time=datetime(2025, 1, 15, 22, 0, 0),
            actual_outcome="home",
            final_score=FinalScore(home=118, away=112),
            markets=[MarketSpec(market_name="Test Market")],
        )
        data = event.to_dict()
        restored = Event.from_dict(data)

        assert restored.event_id == event.event_id
        assert restored.home_team == event.home_team
        assert restored.final_score.home == 118
        assert len(restored.markets) == 1


class TestDataset:
    """Tests for Dataset."""

    def test_duration(self):
        """Duration is calculated correctly."""
        start = datetime(2025, 1, 15, 18, 0, 0)
        end = datetime(2025, 1, 15, 23, 0, 0)
        dataset = Dataset(
            name="Test",
            sport="basketball_nba",
            time_range=(start, end),
            events=[],
            news=[],
        )
        # 5 hours = 18000 seconds
        assert dataset.duration == 5 * 3600

    def test_get_news_for_event(self):
        """Can filter news by event."""
        news = [
            NewsItem(
                timestamp=datetime(2025, 1, 15, 19, 0, 0),
                headline="News 1",
                content="",
                source="other",
                event_id="event_a",
            ),
            NewsItem(
                timestamp=datetime(2025, 1, 15, 19, 30, 0),
                headline="News 2",
                content="",
                source="other",
                event_id="event_b",
            ),
            NewsItem(
                timestamp=datetime(2025, 1, 15, 20, 0, 0),
                headline="News 3",
                content="",
                source="other",
                event_id="event_a",
            ),
        ]
        dataset = Dataset(
            name="Test",
            sport="basketball_nba",
            time_range=(datetime(2025, 1, 15, 18, 0, 0), datetime(2025, 1, 15, 23, 0, 0)),
            events=[],
            news=news,
        )
        event_a_news = dataset.get_news_for_event("event_a")
        assert len(event_a_news) == 2
        assert all(n.event_id == "event_a" for n in event_a_news)


class TestNewsScheduler:
    """Tests for NewsScheduler."""

    def test_subscribe(self):
        """Can subscribe to news."""
        clock = SimulatedClock(sim_start=datetime(2025, 1, 15, 19, 0, 0))
        scheduler = NewsScheduler(clock=clock, news_items=[])
        queue = scheduler.subscribe()
        assert queue is not None

    def test_news_sorted_by_timestamp(self):
        """News items are sorted by timestamp."""
        clock = SimulatedClock(sim_start=datetime(2025, 1, 15, 19, 0, 0))
        news = [
            NewsItem(
                timestamp=datetime(2025, 1, 15, 20, 0, 0),
                headline="Later",
                content="",
                source="other",
            ),
            NewsItem(
                timestamp=datetime(2025, 1, 15, 19, 0, 0),
                headline="Earlier",
                content="",
                source="other",
            ),
        ]
        scheduler = NewsScheduler(clock=clock, news_items=news)
        assert scheduler.news_items[0].headline == "Earlier"
        assert scheduler.news_items[1].headline == "Later"

    def test_get_upcoming(self):
        """Can get upcoming news."""
        clock = SimulatedClock(sim_start=datetime(2025, 1, 15, 19, 0, 0))
        news = [
            NewsItem(
                timestamp=datetime(2025, 1, 15, 19, 10, 0),
                headline="News 1",
                content="",
                source="other",
            ),
            NewsItem(
                timestamp=datetime(2025, 1, 15, 19, 20, 0),
                headline="News 2",
                content="",
                source="other",
            ),
        ]
        scheduler = NewsScheduler(clock=clock, news_items=news)
        upcoming = scheduler.get_upcoming(5)
        assert len(upcoming) == 2


class TestDrainQueue:
    """Tests for drain_queue utility."""

    @pytest.mark.asyncio
    async def test_drain_empty_queue(self):
        """Draining empty queue returns empty list."""
        queue: asyncio.Queue[NewsItem] = asyncio.Queue()
        items = await drain_queue(queue)
        assert items == []

    @pytest.mark.asyncio
    async def test_drain_with_items(self):
        """Draining queue with items returns all items."""
        queue: asyncio.Queue[NewsItem] = asyncio.Queue()
        news1 = NewsItem(
            timestamp=datetime(2025, 1, 15, 19, 0, 0),
            headline="News 1",
            content="",
            source="other",
        )
        news2 = NewsItem(
            timestamp=datetime(2025, 1, 15, 19, 10, 0),
            headline="News 2",
            content="",
            source="other",
        )
        await queue.put(news1)
        await queue.put(news2)

        items = await drain_queue(queue)
        assert len(items) == 2
        assert items[0].headline == "News 1"
        assert items[1].headline == "News 2"
        # Queue should be empty after drain
        assert queue.empty()
