"""Tests for the live sizer (LiveLlmTrader) bookkeeping and price resolution.

Post SYB-210 the trader is an LLM-free sizer: it drains FairValueUpdates from a
persona bus and rebalances mechanically. The analysis-LLM tests moved to
test_analyst.py; the sizer/bus integration tests live in test_analyst.py too.
"""

from datetime import datetime, timezone
from unittest.mock import MagicMock

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
