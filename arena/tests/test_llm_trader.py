"""Tests for sim.llm_trader.LlmTrader."""

from datetime import datetime, timedelta
from unittest.mock import MagicMock

import pytest

from sim.clock import SimulatedClock
from sim.llm_trader import (
    Article,
    LlmTrader,
    PriceSnapshot,
    TradeRecord,
    _describe_order,
    _format_fills,
)
from sybil_client import BuyNo, BuyYes, SellNo, SellYes


def _make_trader(**kwargs):
    """Create an LlmTrader with minimal mocked dependencies."""
    defaults = dict(
        client=MagicMock(),
        account_id=1,
        articles=[],
        clock=SimulatedClock(sim_start=datetime(2026, 1, 1), compression_ratio=60.0),
        api_key="test",
        persona="You are a test trader.",
        market_question="Will X happen by March 31?",
        context="Some context about the market.",
        model_name="test-model",
        name="TestTrader",
        market_ids=[0],
    )
    defaults.update(kwargs)
    return LlmTrader(**defaults)


# ---------------------------------------------------------------------------
# _parse_orders
# ---------------------------------------------------------------------------


def test_parse_orders_buy_yes():
    """Single BUY_YES order is parsed correctly."""
    t = _make_trader()
    text = (
        "ANALYSIS: The market looks undervalued.\n"
        "FAIR_VALUE: 0.60\n"
        "ORDERS: BUY_YES 50 @ 0.55\n"
        "MOTIVATION: Strong signal for yes."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert len(orders) == 1
    order = orders[0]
    assert isinstance(order, BuyYes)
    assert order.quantity == 50
    assert abs(order.limit_price_nanos / 1_000_000_000 - 0.55) < 0.01


def test_parse_orders_buy_no():
    """Single BUY_NO order is parsed correctly."""
    t = _make_trader()
    text = (
        "ANALYSIS: The market is overpriced.\n"
        "FAIR_VALUE: 0.30\n"
        "ORDERS: BUY_NO 100 @ 0.70\n"
        "MOTIVATION: Overvalued yes side."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert len(orders) == 1
    order = orders[0]
    assert isinstance(order, BuyNo)
    assert order.quantity == 100
    assert abs(order.limit_price_nanos / 1_000_000_000 - 0.70) < 0.01


def test_parse_orders_sell():
    """SELL_YES and SELL_NO orders are parsed correctly."""
    t = _make_trader()
    text = (
        "ANALYSIS: Time to take profits.\n"
        "FAIR_VALUE: 0.50\n"
        "ORDERS: SELL_YES 30 @ 0.60, SELL_NO 20 @ 0.55\n"
        "MOTIVATION: Reducing exposure."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert len(orders) == 2
    assert isinstance(orders[0], SellYes)
    assert orders[0].quantity == 30
    assert isinstance(orders[1], SellNo)
    assert orders[1].quantity == 20


def test_parse_orders_multiple():
    """Multiple orders on one line are all parsed."""
    t = _make_trader()
    text = (
        "ANALYSIS: Complex rebalance.\n"
        "FAIR_VALUE: 0.45\n"
        "ORDERS: BUY_YES 50 @ 0.40, SELL_NO 25 @ 0.60, BUY_NO 10 @ 0.55\n"
        "MOTIVATION: Rebalancing portfolio."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert len(orders) == 3
    assert isinstance(orders[0], BuyYes)
    assert isinstance(orders[1], SellNo)
    assert isinstance(orders[2], BuyNo)


def test_parse_orders_hold():
    """HOLD returns empty orders list."""
    t = _make_trader()
    text = (
        "ANALYSIS: No actionable signal.\n"
        "FAIR_VALUE: 0.50\n"
        "ORDERS: HOLD\n"
        "MOTIVATION: Waiting for more information."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert orders == []


def test_parse_orders_dollar_sign():
    """Prices with $ prefix are parsed correctly."""
    t = _make_trader()
    text = (
        "ANALYSIS: Strong signal.\n"
        "FAIR_VALUE: 0.60\n"
        "ORDERS: BUY_YES 100 @ $0.2000\n"
        "MOTIVATION: Buying."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert len(orders) == 1
    assert isinstance(orders[0], BuyYes)
    assert orders[0].quantity == 100
    assert abs(orders[0].limit_price_nanos / 1_000_000_000 - 0.20) < 0.01


def test_parse_orders_malformed():
    """Garbage text returns None (graceful failure)."""
    t = _make_trader()
    text = "This is just random garbage with no structure at all."
    result = t._parse_orders(text)
    assert result is None


def test_parse_orders_extracts_analysis():
    """Verifies analysis field is extracted from ANALYSIS: line."""
    t = _make_trader()
    text = (
        "ANALYSIS: The recent developments suggest increased probability.\n"
        "FAIR_VALUE: 0.65\n"
        "ORDERS: HOLD\n"
        "MOTIVATION: Holding for now."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert "recent developments" in analysis


def test_parse_orders_extracts_fair_value():
    """Verifies fair_value is parsed as a float."""
    t = _make_trader()
    text = (
        "ANALYSIS: Neutral outlook.\n"
        "FAIR_VALUE: 0.35\n"
        "ORDERS: HOLD\n"
        "MOTIVATION: Fair value near market."
    )
    result = t._parse_orders(text)
    assert result is not None
    analysis, fair_value, orders, motivation = result
    assert abs(fair_value - 0.35) < 0.001


# ---------------------------------------------------------------------------
# _validate_orders
# ---------------------------------------------------------------------------


def test_validate_orders_clips_sell():
    """Can't sell more YES than held — quantity is clipped."""
    t = _make_trader()
    # Simulate holding 10 YES and 5 NO
    t.positions = {(0, "YES"): 10, (0, "NO"): 5}
    t.balance_history = [100.0]

    orders = [
        SellYes.at_price(0, 0.60, 50),  # want to sell 50, only hold 10
        SellNo.at_price(0, 0.40, 20),   # want to sell 20, only hold 5
    ]
    block = MagicMock()
    validated = t._validate_orders(orders, block)
    sell_yes = [o for o in validated if isinstance(o, SellYes)]
    sell_no = [o for o in validated if isinstance(o, SellNo)]
    assert len(sell_yes) == 1
    assert sell_yes[0].quantity <= 10
    assert len(sell_no) == 1
    assert sell_no[0].quantity <= 5


def test_validate_orders_clips_buy():
    """Can't buy more than affordable — quantity is clipped."""
    t = _make_trader()
    t.positions = {}
    t.balance_history = [10.0]  # only $10

    orders = [
        BuyYes.at_price(0, 0.50, 1000),  # would cost $500, but only have $10
    ]
    block = MagicMock()
    validated = t._validate_orders(orders, block)
    assert len(validated) == 1
    assert validated[0].quantity < 1000
    # At $0.50 per share with $10, max affordable is 20
    assert validated[0].quantity <= 20


def test_validate_orders_drops_zero():
    """Zero-qty orders after clipping are dropped."""
    t = _make_trader()
    t.positions = {(0, "YES"): 0}  # hold 0 YES
    t.balance_history = [100.0]

    orders = [
        SellYes.at_price(0, 0.60, 50),  # hold 0 → clipped to 0 → dropped
    ]
    block = MagicMock()
    validated = t._validate_orders(orders, block)
    assert len(validated) == 0


def test_validate_orders_clamps_price():
    """Prices outside 0.01-0.99 are clamped."""
    t = _make_trader()
    t.positions = {}
    t.balance_history = [1000.0]

    orders = [
        BuyYes.at_price(0, 0.001, 10),  # price too low
        BuyNo.at_price(0, 1.50, 10),    # price too high
    ]
    block = MagicMock()
    validated = t._validate_orders(orders, block)
    for order in validated:
        price = order.limit_price_nanos / 1_000_000_000
        assert price >= 0.01, f"Price {price} below 0.01"
        assert price <= 0.99, f"Price {price} above 0.99"


# ---------------------------------------------------------------------------
# _drain_arrived_articles
# ---------------------------------------------------------------------------


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

    # Second drain: future article not yet arrived
    arrived2 = t._drain_arrived_articles()
    assert len(arrived2) == 0


# ---------------------------------------------------------------------------
# snapshot / restore
# ---------------------------------------------------------------------------


def test_snapshot_restore():
    """snapshot_state/restore_state preserves trade_log and price_history."""
    t = _make_trader()
    t.trade_log = ["trade1", "trade2"]
    t.price_history = [
        PriceSnapshot(block_height=1, sim_time=datetime(2026, 1, 1), yes_price=0.55),
    ]

    state = t.snapshot_state()
    t2 = _make_trader()
    t2.restore_state(state)

    assert t2.trade_log == ["trade1", "trade2"]
    assert len(t2.price_history) == 1
    assert t2.price_history[0].yes_price == 0.55


# ---------------------------------------------------------------------------
# _describe_order
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# _format_fills
# ---------------------------------------------------------------------------


def test_format_fills_empty():
    """_format_fills returns 'no fills' for empty list."""
    assert _format_fills([BuyYes.at_price(0, 0.5, 10)], []) == "no fills"


# ---------------------------------------------------------------------------
# _build_prompt — multi-article batching
# ---------------------------------------------------------------------------


def _make_article(title="Test Article", source="TestSource", text="Some text"):
    return Article(
        timestamp=datetime(2026, 1, 1, 12, 0),
        title=title,
        source=source,
        url="http://example.com",
        full_text=text,
    )


def test_build_prompt_single_article():
    """Single article prompt uses the original format."""
    t = _make_trader(market_ids=[0])
    t.positions = {}
    t.balance_history = [100.0]
    t.price_history = [PriceSnapshot(block_height=1, sim_time=datetime(2026, 1, 1), yes_price=0.50)]

    block = MagicMock()
    block.clearing_prices = {0: (500_000_000, 500_000_000)}

    art = _make_article(title="Iran talks resume", source="Reuters")
    prompt = t._build_prompt([art], block)
    assert "New article from Reuters:" in prompt
    assert '"Iran talks resume"' in prompt
    assert "this article" in prompt


def test_build_prompt_multiple_articles():
    """Multiple articles are numbered in the prompt."""
    t = _make_trader(market_ids=[0])
    t.positions = {}
    t.balance_history = [100.0]
    t.price_history = []

    block = MagicMock()
    block.clearing_prices = {0: (500_000_000, 500_000_000)}

    arts = [
        _make_article(title="Article A", source="Reuters"),
        _make_article(title="Article B", source="BBC"),
        _make_article(title="Article C", source="AP"),
    ]
    prompt = t._build_prompt(arts, block)
    assert "New articles this batch:" in prompt
    assert '[1] From Reuters: "Article A"' in prompt
    assert '[2] From BBC: "Article B"' in prompt
    assert '[3] From AP: "Article C"' in prompt
    assert "these articles" in prompt


# ---------------------------------------------------------------------------
# TradeRecord — articles list serialization
# ---------------------------------------------------------------------------


def test_trade_record_to_dict_single():
    """TradeRecord.to_dict() with one article has backward-compat fields."""
    art = _make_article(title="Test", source="Src")
    rec = TradeRecord(
        articles=[art],
        analysis="test analysis",
        fair_value=0.55,
        orders=[],
        motivation="test",
        raw_llm_response="raw",
        llm_duration_s=1.0,
        block_height=5,
        sim_time=datetime(2026, 1, 1, 12, 0),
        balance=100.0,
        yes_pos=0,
        no_pos=0,
    )
    d = rec.to_dict()
    assert d["article_title"] == "Test"
    assert d["article_source"] == "Src"
    assert len(d["articles"]) == 1
    assert d["articles"][0]["title"] == "Test"


def test_trade_record_to_dict_multiple():
    """TradeRecord.to_dict() with multiple articles lists all of them."""
    arts = [_make_article(title=f"Art{i}", source=f"Src{i}") for i in range(3)]
    rec = TradeRecord(
        articles=arts,
        analysis="batch analysis",
        fair_value=0.40,
        orders=[],
        motivation="batch",
        raw_llm_response="raw",
        llm_duration_s=2.0,
        block_height=10,
        sim_time=datetime(2026, 1, 1, 14, 0),
        balance=80.0,
        yes_pos=10,
        no_pos=5,
    )
    d = rec.to_dict()
    # Backward compat uses first article
    assert d["article_title"] == "Art0"
    assert d["article_source"] == "Src0"
    # Full list
    assert len(d["articles"]) == 3
    assert d["articles"][2]["title"] == "Art2"
