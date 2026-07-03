"""Tests for live trader bookkeeping."""

from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock

from live.trader import LiveLlmTrader, _order_to_log_dict
from sybil_client import BuyYes


def _make_trader(db=None):
    news_feed = MagicMock()
    news_feed.polymarket_prices.get_price.return_value = 0.55
    market = MagicMock()
    market.id = 7
    market.name = "Test Market"
    market.reference_price_nanos = None
    return LiveLlmTrader(
        client=MagicMock(),
        account_id=1,
        news_feed=news_feed,
        api_key="test",
        persona="Test persona",
        model_name="test-model",
        market_ids=[7],
        markets_info={7: market},
        db=db,
        name="Test Trader",
    )


def test_order_to_log_dict():
    order = BuyYes.at_price(7, 0.55, 12)
    assert _order_to_log_dict(order) == {
        "market_id": 7,
        "side": "BUY_YES",
        "qty": 12,
        "price": 0.55,
    }


def test_record_trade_logs_orders_to_db():
    db = MagicMock()
    trader = _make_trader(db=db)
    trader.balance_history = [500.0]
    trader.positions = {(7, "YES"): 3}

    order = BuyYes.at_price(7, 0.55, 12)
    trader._record_trade(
        market_id=7,
        market_name="Test Market",
        fair_value=0.61,
        orders=[order],
        motivation="Kelly rebalance to target position",
        analysis="",
        raw_llm_response="",
        llm_duration_s=0.0,
        market_price=0.55,
        block_height=42,
        timestamp=datetime(2026, 1, 1, tzinfo=timezone.utc),
    )

    assert len(trader.trade_log[7]) == 1
    db.log_decision.assert_called_once()
    payload = db.log_decision.call_args.kwargs
    assert payload["market_id"] == 7
    assert payload["orders"] == [{
        "market_id": 7,
        "side": "BUY_YES",
        "qty": 12,
        "price": 0.55,
    }]


def _price_block():
    from sybil_client.types import Block

    return Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={7: (900_000_000, 100_000_000)},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )


def test_market_price_prefers_fresh_polymarket_poll():
    # AR-1: the fresh Polymarket poll wins over the frozen startup snapshot so
    # sizing sees the same price shown to the LLM.
    trader = _make_trader()
    trader.markets_info[7].reference_price_nanos = 120_000_000  # stale snapshot
    trader.news_feed.polymarket_prices.get_price.return_value = 0.55  # fresh poll

    assert trader._get_market_price(7, _price_block()) == 0.55


def test_market_price_falls_back_to_reference_snapshot():
    # AR-1: when the fresh poll is unavailable, fall back to the snapshot.
    trader = _make_trader()
    trader.markets_info[7].reference_price_nanos = 120_000_000
    trader.news_feed.polymarket_prices.get_price.return_value = None

    assert trader._get_market_price(7, _price_block()) == 0.12


def test_market_price_falls_back_to_clearing_price():
    # AR-1: with neither poll nor snapshot, use on-chain clearing.
    trader = _make_trader()
    trader.markets_info[7].reference_price_nanos = None
    trader.news_feed.polymarket_prices.get_price.return_value = None

    assert trader._get_market_price(7, _price_block()) == 0.90


def test_observed_market_prices_do_not_invent_default_prices():
    from sybil_client.types import Block

    trader = _make_trader()
    trader.news_feed.polymarket_prices.get_price.return_value = 0
    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    assert trader._observed_market_prices(block) == {}


def test_parse_fair_value_tolerates_trailing_dot():
    trader = _make_trader()

    parsed = trader._parse_fair_value(
        "FAIR_VALUE: 0.85.\n"
        "MOTIVATION: Strong new evidence.\n"
        "ANALYSIS: The article directly updates the market."
    )

    assert parsed == (
        0.85,
        "Strong new evidence.",
        "The article directly updates the market.",
    )


def test_parse_fair_value_invalid_number_returns_none():
    trader = _make_trader()

    assert trader._parse_fair_value("FAIR_VALUE: 0.8.5\nMOTIVATION: bad") is None


def _make_multi_market_trader(market_ids):
    from live.news_feed import LiveArticle

    news_feed = MagicMock()
    news_feed.polymarket_prices.get_price.return_value = 0.55
    article = LiveArticle(
        url="http://x/a",
        title="Something happened",
        source="src",
        published=datetime(2026, 1, 1, tzinfo=timezone.utc),
        full_text="Body text.",
    )
    news_feed.drain = AsyncMock(return_value=[article])

    markets_info = {}
    for mid in market_ids:
        m = MagicMock()
        m.id = mid
        m.name = f"Market {mid}"
        m.description = ""
        m.resolution_criteria = ""
        m.reference_price_nanos = None
        markets_info[mid] = m

    return LiveLlmTrader(
        client=MagicMock(),
        account_id=1,
        news_feed=news_feed,
        api_key="test",
        persona="Test persona",
        model_name="test-model",
        market_ids=list(market_ids),
        markets_info=markets_info,
        min_llm_interval_s=1000.0,
        name="Burst Trader",
    )


def _block():
    from sybil_client.types import Block

    return Block(
        height=2,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )


async def test_llm_interval_gates_per_call_not_per_block():
    # AR-6: several markets have fresh articles in a single block, but the
    # min interval must cap the trader to one LLM call — not a burst.
    trader = _make_multi_market_trader([7, 8, 9])
    trader.balance_history = [500.0]
    trader._observed_first_block = True
    trader._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.60\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    await trader.on_block(_block())

    assert trader._call_llm.call_count == 1


async def test_llm_interval_allows_one_call_per_elapsed_interval():
    # AR-6: after the interval elapses (simulated by resetting the timestamp),
    # the next block is allowed exactly one more call.
    trader = _make_multi_market_trader([7, 8])
    trader.balance_history = [500.0]
    trader._observed_first_block = True
    trader._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.60\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    await trader.on_block(_block())
    assert trader._call_llm.call_count == 1

    trader._last_llm_call = 0.0  # interval elapsed
    await trader.on_block(_block())
    assert trader._call_llm.call_count == 2


def test_db_persists_and_reattaches_bot_account(tmp_path):
    # AR-3: (persona, strategy) -> account_id survives so restarts reattach.
    from live.db import DecisionDB

    db = DecisionDB(str(tmp_path / "decisions.db"))
    try:
        assert db.get_bot_account_id("news_trader", "Kelly") is None

        db.save_bot_account_id("news_trader", "Kelly", 101)
        db.save_bot_account_id("news_trader", "Flat", 102)
        assert db.get_bot_account_id("news_trader", "Kelly") == 101
        assert db.get_bot_account_id("news_trader", "Flat") == 102

        # Upsert: re-saving the same pair overwrites rather than duplicating.
        db.save_bot_account_id("news_trader", "Kelly", 201)
        assert db.get_bot_account_id("news_trader", "Kelly") == 201
    finally:
        db.close()
