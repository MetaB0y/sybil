"""Tests for the live sizer (LiveLlmTrader) bookkeeping and price resolution.

Post SYB-210 the trader is an LLM-free sizer: it drains FairValueUpdates from a
persona bus and rebalances mechanically. The analysis-LLM tests moved to
test_analyst.py; the sizer/bus integration tests live in test_analyst.py too.
"""

from datetime import datetime, timedelta, timezone
from unittest.mock import MagicMock

import pytest

from live.fair_value_bus import FairValueBus, FairValueUpdate
from live.strategy import FlatStrategy
from live.trader import LiveLlmTrader, _order_to_log_dict
from sybil_client import BuyYes, SellYes


def _make_trader(db=None, **kwargs):
    news_feed = MagicMock()
    news_feed.polymarket_prices.get_price.return_value = 0.55
    market = MagicMock()
    market.id = 7
    market.name = "Test Market"
    market.reference_price_nanos = None
    market.status = "Active"
    market.category = "Politics"
    market.tags = ["polymarket", "elections"]
    return LiveLlmTrader(
        client=MagicMock(),
        account_id=1,
        news_feed=news_feed,
        market_ids=[7],
        markets_info={7: market},
        db=db,
        name="Test Trader",
        **kwargs,
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
        restate="YES resolves if the named event occurs.",
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
    assert payload["restate"] == "YES resolves if the named event occurs."
    assert payload["orders"] == [
        {
            "market_id": 7,
            "side": "BUY_YES",
            "qty": 12,
            "price": 0.55,
        }
    ]


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


def test_decision_db_migrates_old_decisions_table_with_stage_metadata(tmp_path):
    from live.db import DecisionDB
    import sqlite3

    db_path = tmp_path / "old.db"
    conn = sqlite3.connect(db_path)
    conn.execute(
        """CREATE TABLE decisions (
               id INTEGER PRIMARY KEY, trader_name TEXT, timestamp TEXT, orders TEXT
           )"""
    )
    conn.commit()
    conn.close()

    db = DecisionDB(str(db_path))
    try:
        columns = {row[1] for row in db.conn.execute("PRAGMA table_info(decisions)")}
        assert "rejection_reason" in columns
        assert "restate" in columns
        assert "analysis_batch_id" in columns
        assert "analysis_reference_price" in columns
    finally:
        db.close()


def test_hard_expired_fv_exits_existing_position_and_logs_context():
    now = datetime(2026, 1, 1, 12, tzinfo=timezone.utc)
    trader = _make_trader(
        fair_value_ttl_s=10.0,
        fair_value_half_life_s=20.0,
        fair_value_hard_expiry_s=100.0,
        now_fn=lambda: now,
        monotonic_fn=lambda: 1000.0,
    )
    trader.balance_history = [500.0]
    trader.positions = {(7, "YES"): 10}
    trader.fair_values[7] = 0.80
    trader.fair_value_timestamps[7] = now - timedelta(seconds=100)
    trader.fair_value_confidences[7] = 0.9

    orders = trader._rebalance_all(_price_block(), now)

    assert len(orders) == 1
    assert isinstance(orders[0], SellYes)
    assert orders[0].quantity == 10
    context = trader._latest_rebalance_context[7]
    assert context.raw_fair_value == 0.80
    assert context.effective_fair_value is None
    assert context.age_s == 100
    assert context.confidence == 0.9


@pytest.mark.parametrize(
    ("fair_value", "price", "balance", "position", "age_s", "expected"),
    [
        (0.56, 0.55, 500.0, 0, 0, "below_min_edge"),
        (0.80, 0.55, 0.0, 0, 0, "insufficient_cash"),
        (0.80, 0.55, 500.0, 10, 0, "hold_position"),
        (0.80, 0.55, 500.0, 0, 100, "fv_expired"),
        (0.80, 0.96, 500.0, 0, 0, "resolved"),
    ],
)
async def test_rebalance_records_no_order_rejection_reason(
    fair_value, price, balance, position, age_s, expected
):
    now = datetime(2026, 1, 1, 12, tzinfo=timezone.utc)
    bus = FairValueBus("test")
    db = MagicMock()
    trader = _make_trader(
        db=db,
        strategy=FlatStrategy(),
        fair_value_bus=bus,
        fair_value_ttl_s=10.0,
        fair_value_hard_expiry_s=100.0,
        now_fn=lambda: now,
        monotonic_fn=lambda: 1000.0,
    )
    trader._observed_first_block = True
    trader.balance_history = [balance]
    if position:
        trader.positions = {(7, "YES"): position}
    trader.news_feed.polymarket_prices.get_price.return_value = price
    await bus.publish(
        FairValueUpdate(
            market_id=7,
            persona_key="test",
            fair_value=fair_value,
            motivation="fixture",
            analysis="fixture analysis",
            restate="YES resolves if the fixture event occurs.",
            ts=now - timedelta(seconds=age_s),
        )
    )

    orders = await trader.on_block(_price_block())

    assert orders == []
    assert db.log_decision.call_args.kwargs["rejection_reason"] == expected
    assert db.log_decision.call_args.kwargs["orders"] == []
    assert db.log_decision.call_args.kwargs["restate"] == (
        "YES resolves if the fixture event occurs."
    )


@pytest.mark.asyncio
async def test_timer_rebalance_does_not_duplicate_forecast_decision():
    now = datetime(2026, 1, 1, 12, tzinfo=timezone.utc)
    monotonic = 1_000.0
    bus = FairValueBus("test")
    db = MagicMock()
    trader = _make_trader(
        db=db,
        strategy=FlatStrategy(),
        fair_value_bus=bus,
        now_fn=lambda: now,
        monotonic_fn=lambda: monotonic,
    )
    trader._observed_first_block = True
    trader.balance_history = [500.0]
    await bus.publish(
        FairValueUpdate(
            market_id=7,
            persona_key="test",
            fair_value=0.80,
            motivation="fresh evidence",
            analysis="one forecast",
            analysis_batch_id="batch-1",
            ts=now,
        )
    )

    await trader.on_block(_price_block())
    assert db.log_decision.call_count == 1

    monotonic += trader.strategy.rebalance_interval_s
    await trader.on_block(_price_block())

    assert db.log_decision.call_count == 1
