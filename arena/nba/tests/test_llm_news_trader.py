"""Tests for LLMNewsTrader - all LLM calls are mocked."""

import asyncio
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from backtest.clock import SimulatedClock
from backtest.dataset import NewsItem
from bots.llm_news_trader import (
    CONTRARIAN_SYSTEM_PROMPT,
    DEFAULT_SYSTEM_PROMPT,
    LLMNewsTrader,
    _build_prompt,
    _parse_llm_response,
)


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
def trader(clock, event_market_map):
    client = AsyncMock()
    t = LLMNewsTrader(
        client=client,
        account_id=0,
        clock=clock,
        name="TestLLM",
        market_ids=[0, 1],
        event_market_map=event_market_map,
        provider="anthropic",
        model_name="claude-sonnet-4-5-20250929",
        api_key="test-key",
        min_blocks_between_calls=2,
    )
    return t


def make_news(event_id, source="in_game", headline="Score update", **meta):
    return NewsItem(
        timestamp=datetime(2025, 1, 15, 20, 0, tzinfo=timezone.utc),
        headline=headline,
        content="Test content",
        source=source,
        event_id=event_id,
        metadata=meta,
    )


# --- Response Parsing Tests ---

class TestParseResponse:
    def test_valid_json(self):
        result = _parse_llm_response(
            '{"market_0": 0.65, "market_1": 0.30}',
            ["market_0", "market_1"],
        )
        assert result == {"market_0": 0.65, "market_1": 0.30}

    def test_markdown_fenced_json(self):
        result = _parse_llm_response(
            '```json\n{"market_0": 0.55, "market_1": 0.45}\n```',
            ["market_0", "market_1"],
        )
        assert result == {"market_0": 0.55, "market_1": 0.45}

    def test_markdown_fenced_no_lang(self):
        result = _parse_llm_response(
            '```\n{"market_0": 0.7}\n```',
            ["market_0"],
        )
        assert result == {"market_0": 0.7}

    def test_malformed_json_returns_none(self):
        assert _parse_llm_response("not json at all", ["market_0"]) is None

    def test_empty_string_returns_none(self):
        assert _parse_llm_response("", ["market_0"]) is None

    def test_clamping_low(self):
        result = _parse_llm_response(
            '{"market_0": 0.001}',
            ["market_0"],
        )
        assert result["market_0"] == 0.01

    def test_clamping_high(self):
        result = _parse_llm_response(
            '{"market_0": 1.0}',
            ["market_0"],
        )
        assert result["market_0"] == 0.99

    def test_clamping_negative(self):
        result = _parse_llm_response(
            '{"market_0": -0.5}',
            ["market_0"],
        )
        assert result["market_0"] == 0.01

    def test_missing_keys_partial_result(self):
        result = _parse_llm_response(
            '{"market_0": 0.6}',
            ["market_0", "market_1"],
        )
        assert result == {"market_0": 0.6}

    def test_all_keys_missing_returns_none(self):
        result = _parse_llm_response(
            '{"other_key": 0.5}',
            ["market_0"],
        )
        assert result is None

    def test_non_dict_returns_none(self):
        assert _parse_llm_response("[0.5, 0.3]", ["market_0"]) is None

    def test_non_numeric_value_skipped(self):
        result = _parse_llm_response(
            '{"market_0": "high", "market_1": 0.4}',
            ["market_0", "market_1"],
        )
        assert result == {"market_1": 0.4}


# --- Prompt Building Tests ---

class TestBuildPrompt:
    def test_includes_market_keys(self):
        event_news = {"ev1": ["[Q1 END] 28 - 25"]}
        event_info = {
            "ev1": {"home_team": "Boston", "away_team": "LA", "market_key": "market_0"},
        }
        market_prices = {"market_0": 0.62}

        prompt = _build_prompt(event_news, event_info, market_prices)
        assert "market_0" in prompt
        assert "Boston" in prompt
        assert "62.0%" in prompt
        assert "Q1 END" in prompt

    def test_multiple_events(self):
        event_news = {
            "ev1": ["[Q2 END] 58 - 54"],
            "ev2": ["[INJURY] Curry out"],
        }
        event_info = {
            "ev1": {"home_team": "BOS", "away_team": "LAL", "market_key": "market_0"},
            "ev2": {"home_team": "GSW", "away_team": "MIA", "market_key": "market_1"},
        }
        market_prices = {"market_0": 0.6, "market_1": 0.4}

        prompt = _build_prompt(event_news, event_info, market_prices)
        assert "market_0" in prompt
        assert "market_1" in prompt
        assert "BOS" in prompt
        assert "GSW" in prompt

    def test_no_news_for_event(self):
        event_info = {
            "ev1": {"home_team": "BOS", "away_team": "LAL", "market_key": "market_0"},
        }
        prompt = _build_prompt({}, event_info, {"market_0": 0.5})
        assert "No updates yet" in prompt


# --- News Accumulation Tests ---

class TestNewsAccumulation:
    @pytest.mark.asyncio
    async def test_news_accumulates_per_event(self, trader):
        news1 = make_news(
            "nba_bos_lal_0115", "in_game", "Q1 Score",
            quarter=1, home_score=30, away_score=27,
        )
        news2 = make_news(
            "nba_bos_lal_0115", "in_game", "Q2 Score",
            quarter=2, home_score=58, away_score=54,
        )

        await trader.on_news(news1)
        await trader.on_news(news2)

        event_news = trader._event_news["nba_bos_lal_0115"]
        assert len(event_news) == 2
        # Most recent first
        assert "Q2" in event_news[0]
        assert "Q1" in event_news[1]

    @pytest.mark.asyncio
    async def test_news_from_different_events(self, trader):
        news1 = make_news("nba_bos_lal_0115", "lineup", "Lineups",
                          home_team="Boston Celtics", away_team="Los Angeles Lakers")
        news2 = make_news("nba_gsw_mia_0115", "lineup", "Lineups",
                          home_team="Golden State Warriors", away_team="Miami Heat")

        await trader.on_news(news1)
        await trader.on_news(news2)

        assert len(trader._event_news["nba_bos_lal_0115"]) == 1
        assert len(trader._event_news["nba_gsw_mia_0115"]) == 1

    @pytest.mark.asyncio
    async def test_news_sets_update_flag(self, trader):
        assert not trader._needs_llm_update

        await trader.on_news(make_news("nba_bos_lal_0115"))
        assert trader._needs_llm_update

    @pytest.mark.asyncio
    async def test_unknown_event_ignored(self, trader):
        await trader.on_news(make_news("unknown_event"))
        assert "unknown_event" in trader._event_news  # still accumulated
        assert trader._needs_llm_update


# --- Rate Limiting Tests ---

class TestRateLimiting:
    @pytest.mark.asyncio
    async def test_respects_min_blocks(self, trader):
        trader._needs_llm_update = True
        trader._blocks_since_last_call = 0  # Not enough blocks yet

        block = MagicMock()
        block.clearing_prices = {0: (500_000_000, 500_000_000)}
        block.fills = []

        # Mock _update_state to avoid HTTP calls
        trader._update_state = AsyncMock()

        orders = await trader.on_block(block)

        # LLM should not have been called (blocks_since < min_blocks)
        assert trader._llm_task is None

    @pytest.mark.asyncio
    async def test_calls_after_enough_blocks(self, trader):
        trader._needs_llm_update = True
        trader._blocks_since_last_call = 5  # Enough blocks

        block = MagicMock()
        block.clearing_prices = {0: (500_000_000, 500_000_000)}
        block.fills = []

        trader._update_state = AsyncMock()

        with patch.object(trader, "_update_probabilities", new_callable=AsyncMock) as mock_update:
            await trader.on_block(block)

            # LLM task should have been created
            assert trader._llm_task is not None
            # Wait for it
            await trader._llm_task
            mock_update.assert_called_once()


# --- News Formatting Tests ---

class TestNewsFormatting:
    def test_in_game_format(self, trader):
        news = make_news("ev", "in_game", "Score", quarter=2, home_score=58, away_score=54)
        formatted = trader._format_news_line(news)
        assert "[Q2 END]" in formatted
        assert "58" in formatted
        assert "54" in formatted

    def test_final_format(self, trader):
        news = make_news("ev", "in_game", "Final", final=True, home_score=118, away_score=112)
        formatted = trader._format_news_line(news)
        assert "[FINAL]" in formatted

    def test_injury_format(self, trader):
        news = make_news("ev", "injury", "Curry hurt", player="Stephen Curry", status="out")
        formatted = trader._format_news_line(news)
        assert "[INJURY]" in formatted
        assert "Curry" in formatted
        assert "out" in formatted

    def test_lineup_format(self, trader):
        news = make_news("ev", "lineup", "Starting lineups")
        formatted = trader._format_news_line(news)
        assert "[LINEUP]" in formatted


# --- System Prompt Variants ---

class TestBeliefsSyncing:
    @pytest.mark.asyncio
    async def test_beliefs_synced_after_llm_update(self, trader):
        """After _update_probabilities, beliefs should reflect the LLM output."""
        trader.clock.start()

        # Mock _call_llm to return known probabilities
        async def fake_call_llm(prompt):
            return '{"market_0": 0.72, "market_1": 0.35}'

        trader._call_llm = fake_call_llm

        await trader._update_probabilities({"market_0": 0.5, "market_1": 0.5})

        # Check beliefs are synced
        assert 0 in trader.beliefs
        assert 1 in trader.beliefs
        assert abs(trader.beliefs[0].probability - 0.72) < 0.01
        assert abs(trader.beliefs[1].probability - 0.35) < 0.01


class TestPromptVariants:
    def test_default_prompt(self):
        assert "NBA analyst" in DEFAULT_SYSTEM_PROMPT
        assert "JSON" in DEFAULT_SYSTEM_PROMPT

    def test_contrarian_prompt(self):
        assert "contrarian" in CONTRARIAN_SYSTEM_PROMPT
        assert "overreacts" in CONTRARIAN_SYSTEM_PROMPT
