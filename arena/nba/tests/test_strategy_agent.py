"""Tests for StrategyAgent, MarketView, and shared format_news_line."""

import asyncio
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock

import pytest

from backtest.clock import SimulatedClock
from backtest.dataset import NewsItem
from bots.strategy_agent import MarketView, StrategyAgent, format_news_line


# --- Fixtures ---


@pytest.fixture
def clock():
    return SimulatedClock(
        sim_start=datetime(2025, 1, 15, 18, 30, tzinfo=timezone.utc),
        compression_ratio=60.0,
    )


@pytest.fixture
def event_market_map():
    return {
        "nba_bos_lal_0115": 0,
        "nba_gsw_mia_0115": 1,
    }


@pytest.fixture
def simple_strategy():
    """A strategy that returns fixed estimates."""
    def strategy(markets):
        return {name: 0.7 for name in markets}
    return strategy


@pytest.fixture
def agent(clock, event_market_map, simple_strategy):
    client = AsyncMock()
    return StrategyAgent(
        client=client,
        account_id=0,
        clock=clock,
        name="TestStrategy",
        market_ids=[0, 1],
        event_market_map=event_market_map,
        strategy_fn=simple_strategy,
        edge_threshold=0.05,
        order_size=5,
        max_position=50,
    )


def make_news(event_id, source="in_game", headline="Score update", content="content", **meta):
    return NewsItem(
        timestamp=datetime(2025, 1, 15, 20, 0, tzinfo=timezone.utc),
        headline=headline,
        content=content,
        source=source,
        event_id=event_id,
        metadata=meta,
    )


# --- format_news_line Tests ---


class TestFormatNewsLine:
    def test_in_game_format(self):
        news = make_news("ev", "in_game", "Score", quarter=2, home_score=58, away_score=54)
        formatted = format_news_line(news)
        assert "[Q2 END]" in formatted
        assert "58" in formatted
        assert "54" in formatted

    def test_final_format(self):
        news = make_news("ev", "in_game", "Final", final=True, home_score=118, away_score=112)
        formatted = format_news_line(news)
        assert "[FINAL]" in formatted
        assert "118" in formatted

    def test_injury_format(self):
        news = make_news("ev", "injury", "Curry hurt", player="Stephen Curry", status="out")
        formatted = format_news_line(news)
        assert "[INJURY]" in formatted
        assert "Curry" in formatted
        assert "out" in formatted

    def test_lineup_format(self):
        news = make_news("ev", "lineup", "Starting lineups", content="Starters announced")
        formatted = format_news_line(news)
        assert "[LINEUP]" in formatted

    def test_other_source(self):
        news = make_news("ev", "weather", "Rain delay")
        formatted = format_news_line(news)
        assert "[WEATHER]" in formatted

    def test_injury_falls_back_to_severity(self):
        news = make_news("ev", "injury", "Player hurt", player="LeBron", severity="serious")
        formatted = format_news_line(news)
        assert "serious" in formatted


# --- MarketView Tests ---


class TestMarketView:
    def test_creation(self):
        view = MarketView(name="Celtics vs Lakers", price=0.62, news=["[Q1 END] 28 - 25"], position=5)
        assert view.name == "Celtics vs Lakers"
        assert view.price == 0.62
        assert len(view.news) == 1
        assert view.position == 5

    def test_frozen(self):
        view = MarketView(name="Test", price=0.5, news=[], position=0)
        with pytest.raises(AttributeError):
            view.price = 0.6


# --- StrategyAgent Tests ---


class TestStrategyAgent:
    def test_requires_strategy_fn(self, clock, event_market_map):
        with pytest.raises(ValueError, match="strategy_fn"):
            StrategyAgent(
                client=AsyncMock(),
                account_id=0,
                clock=clock,
                market_ids=[0],
                event_market_map=event_market_map,
            )

    @pytest.mark.asyncio
    async def test_news_accumulates(self, agent):
        news1 = make_news(
            "nba_bos_lal_0115", "in_game", "Q1",
            quarter=1, home_score=28, away_score=25,
        )
        news2 = make_news(
            "nba_bos_lal_0115", "in_game", "Q2",
            quarter=2, home_score=58, away_score=54,
        )
        await agent.on_news(news1)
        await agent.on_news(news2)

        event_news = agent._event_news["nba_bos_lal_0115"]
        assert len(event_news) == 2
        # Most recent first
        assert "Q2" in event_news[0]
        assert "Q1" in event_news[1]

    @pytest.mark.asyncio
    async def test_team_names_extracted(self, agent):
        news = make_news(
            "nba_bos_lal_0115", "lineup", "Lineups",
            content="Starting lineups",
            home_team="Boston Celtics",
            away_team="Los Angeles Lakers",
        )
        await agent.on_news(news)

        assert agent._event_display_names["nba_bos_lal_0115"] == "Boston Celtics vs Los Angeles Lakers"
        display = "Boston Celtics vs Los Angeles Lakers"
        assert agent._display_to_market[display] == 0
        assert agent._market_to_display[0] == display

    @pytest.mark.asyncio
    async def test_on_block_calls_strategy(self, agent):
        called_with = {}

        def spy_strategy(markets):
            called_with.update(markets)
            return {"nba_bos_lal_0115": 0.7}

        agent.strategy_fn = spy_strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {
            0: (500_000_000, 500_000_000),
            1: (500_000_000, 500_000_000),
        }
        block.fills = []

        orders = await agent.on_block(block)

        # Strategy was called with MarketViews
        assert len(called_with) == 2

    @pytest.mark.asyncio
    async def test_on_block_generates_buy_yes(self, agent):
        """When strategy estimate > price + threshold, should buy YES."""
        def bullish_strategy(markets):
            return {name: 0.8 for name in markets}  # All bullish

        agent.strategy_fn = bullish_strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {
            0: (500_000_000, 500_000_000),  # price = 0.50
            1: (500_000_000, 500_000_000),
        }
        block.fills = []

        orders = await agent.on_block(block)
        # Edge = 0.8 - 0.5 = 0.3 > 0.05 threshold → should buy YES
        assert len(orders) == 2

    @pytest.mark.asyncio
    async def test_on_block_generates_buy_no(self, agent):
        """When strategy estimate < price - threshold, should buy NO."""
        def bearish_strategy(markets):
            return {name: 0.2 for name in markets}

        agent.strategy_fn = bearish_strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {
            0: (500_000_000, 500_000_000),
            1: (500_000_000, 500_000_000),
        }
        block.fills = []

        orders = await agent.on_block(block)
        assert len(orders) == 2

    @pytest.mark.asyncio
    async def test_no_trade_within_threshold(self, agent):
        """No orders when edge < threshold."""
        def neutral_strategy(markets):
            return {name: view.price + 0.01 for name, view in markets.items()}

        agent.strategy_fn = neutral_strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {
            0: (500_000_000, 500_000_000),
            1: (500_000_000, 500_000_000),
        }
        block.fills = []

        orders = await agent.on_block(block)
        assert len(orders) == 0

    @pytest.mark.asyncio
    async def test_strategy_error_returns_empty(self, agent):
        """Buggy strategies don't crash the agent."""
        def broken_strategy(markets):
            raise RuntimeError("oops")

        agent.strategy_fn = broken_strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {0: (500_000_000, 500_000_000)}
        block.fills = []

        orders = await agent.on_block(block)
        assert orders == []

    @pytest.mark.asyncio
    async def test_beliefs_synced(self, agent):
        """Strategy estimates should be synced to agent.beliefs."""
        agent.clock.start()

        def strategy(markets):
            return {name: 0.75 for name in markets}

        agent.strategy_fn = strategy
        agent._update_state = AsyncMock()

        block = MagicMock()
        block.clearing_prices = {
            0: (500_000_000, 500_000_000),
            1: (500_000_000, 500_000_000),
        }
        block.fills = []

        await agent.on_block(block)

        # Beliefs should be synced for all traded markets
        assert 0 in agent.beliefs
        assert 1 in agent.beliefs
        assert abs(agent.beliefs[0].probability - 0.75) < 0.01
        assert abs(agent.beliefs[1].probability - 0.75) < 0.01

    @pytest.mark.asyncio
    async def test_ignores_news_without_event_id(self, agent):
        news = make_news(None, "other", "General news")
        await agent.on_news(news)
        assert len(agent._event_news) == 0
