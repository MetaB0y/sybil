"""Tests for sim.news_trader.NewsTrader."""

from datetime import datetime, timedelta
from unittest.mock import MagicMock

import pytest

from sim.clock import SimulatedClock
from sim.news_trader import (
    Article,
    NewsTrader,
    PriceSnapshot,
    TradeRecord,
    _describe_order,
    _format_fills,
)
from sybil_client import BuyNo, BuyYes, SellNo, SellYes


def _make_trader(**kwargs):
    """Create a NewsTrader with minimal mocked dependencies."""
    defaults = dict(
        client=MagicMock(),
        account_id=1,
        articles=[],
        clock=SimulatedClock(sim_start=datetime(2026, 1, 1), compression_ratio=60.0),
        api_key="test",
        persona="You are a test trader.",
        analysis_question="What does this article signal?",
        model_name="test-model",
        name="TestTrader",
        market_ids=[0],
    )
    defaults.update(kwargs)
    return NewsTrader(**defaults)


def test_update_belief_low_conviction():
    """Conviction=1 updates with minimum belief strength."""
    t = _make_trader()
    t._belief_alpha = 5.0
    t._belief_beta = 5.0
    t._belief_initialized = True

    # Default range (1.0, 6.0): conviction=1 → strength=1.0
    belief = t._update_belief(0.8, 1)
    # alpha += 1.0*0.8 = 0.8, beta += 1.0*0.2 = 0.2
    assert abs(t._belief_alpha - 5.8) < 0.01
    assert abs(t._belief_beta - 5.2) < 0.01
    assert abs(belief - 5.8 / 11.0) < 0.01


def test_update_belief_high_conviction():
    """Conviction=10 updates with maximum belief strength."""
    t = _make_trader()
    t._belief_alpha = 5.0
    t._belief_beta = 5.0
    t._belief_initialized = True

    # Default range (1.0, 6.0): conviction=10 → strength=6.0
    belief = t._update_belief(0.9, 10)
    # alpha += 6*0.9 = 5.4, beta += 6*0.1 = 0.6
    assert abs(t._belief_alpha - 10.4) < 0.01
    assert abs(t._belief_beta - 5.6) < 0.01


def test_belief_weight_cap_rescaling():
    """When total weight exceeds cap, alpha/beta are rescaled before update."""
    t = _make_trader(strategy={"belief_weight_cap": 10})
    t._belief_alpha = 8.0
    t._belief_beta = 8.0
    t._belief_initialized = True

    # Total = 16 > 10, so rescale to 10 first
    # conviction=1 → strength=1.0 (default range)
    t._update_belief(0.5, 1)
    # After rescale: alpha = 8*10/16 = 5, beta = 5
    # Then: alpha += 1.0*0.5 = 0.5, beta += 1.0*0.5 = 0.5
    assert abs(t._belief_alpha - 5.5) < 0.01
    assert abs(t._belief_beta - 5.5) < 0.01


def test_drain_arrived_articles():
    """Articles arriving before clock.now() are drained in order."""
    t0 = datetime(2026, 1, 1, 10, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=60.0)

    articles = [
        Article(timestamp=t0 - timedelta(hours=1), title="Past", source="s", url="u", full_text="t"),
        Article(timestamp=t0, title="Now", source="s", url="u", full_text="t"),
        Article(timestamp=t0 + timedelta(hours=1), title="Future", source="s", url="u", full_text="t"),
    ]

    t = _make_trader(articles=articles, clock=clock)
    arrived = t._drain_arrived_articles()
    assert len(arrived) == 2
    assert arrived[0].title == "Past"
    assert arrived[1].title == "Now"

    # Second drain: no more articles (future one not yet arrived)
    arrived2 = t._drain_arrived_articles()
    assert len(arrived2) == 0


def test_snapshot_restore_roundtrip():
    """snapshot_state/restore_state preserves belief and trade log."""
    t = _make_trader()
    t._belief_alpha = 3.0
    t._belief_beta = 7.0
    t._belief_initialized = True
    t.trade_log = ["dummy"]
    t.price_history = ["price"]

    state = t.snapshot_state()
    t2 = _make_trader()
    t2.restore_state(state)

    assert t2._belief_alpha == 3.0
    assert t2._belief_beta == 7.0
    assert t2._belief_initialized is True
    assert t2.trade_log == ["dummy"]
    assert t2.price_history == ["price"]


def test_describe_order():
    """_describe_order formats orders correctly."""
    o = BuyYes.at_price(0, 0.55, 100)
    desc = _describe_order(o)
    assert "BuyYes" in desc
    assert "100" in desc
    assert "$0.55" in desc

    o2 = SellNo.at_price(0, 0.40, 50)
    desc2 = _describe_order(o2)
    assert "SellNo" in desc2
    assert "50" in desc2


def test_format_fills_empty():
    """_format_fills returns 'no fills' for empty list."""
    assert _format_fills([BuyYes.at_price(0, 0.5, 10)], []) == "no fills"
