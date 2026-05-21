"""Tests for live trader bookkeeping."""

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


def test_market_price_prefers_api_reference_price():
    from sybil_client.types import Block

    trader = _make_trader()
    trader.markets_info[7].reference_price_nanos = 120_000_000
    trader.news_feed.polymarket_prices.get_price.return_value = 0.55
    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={7: (900_000_000, 100_000_000)},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    assert trader._get_market_price(7, block) == 0.12


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
