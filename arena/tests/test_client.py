"""Tests for sybil_client."""

import pytest

from sybil_client import BuyNo, BuyYes, SellNo, SellYes
from sybil_client.types import NANOS_PER_DOLLAR, Account, Block, Fill, Market, Position


class TestTypes:
    """Test type conversions and properties."""

    def test_account_balance_dollars(self):
        account = Account(id=1, balance_nanos=50_000_000_000, positions=[])
        assert account.balance_dollars == 50.0

    def test_account_position(self):
        positions = [
            Position(market_id=0, outcome="YES", quantity=10),
            Position(market_id=0, outcome="NO", quantity=5),
            Position(market_id=1, outcome="YES", quantity=3),
        ]
        account = Account(id=1, balance_nanos=0, positions=positions)

        assert account.position(0, "YES") == 10
        assert account.position(0, "NO") == 5
        assert account.position(1, "YES") == 3
        assert account.position(1, "NO") == 0  # Not present
        assert account.position(2, "YES") == 0  # Market not present

    def test_market_prices(self):
        market = Market(
            id=0,
            name="Test",
            yes_price_nanos=600_000_000,
            no_price_nanos=400_000_000,
            status="active",
        )
        assert market.yes_price == 0.6
        assert market.no_price == 0.4

    def test_fill_price(self):
        fill = Fill(order_id=1, fill_qty=10, fill_price_nanos=550_000_000)
        assert fill.fill_price == 0.55

    def test_block_price_for(self):
        block = Block(
            height=1,
            parent_hash="abc",
            state_root="def",
            fills=[],
            clearing_prices={0: (600_000_000, 400_000_000)},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        assert block.price_for(0) == (0.6, 0.4)
        assert block.price_for(1) is None


class TestOrderSpecs:
    """Test order specification helpers."""

    def test_buy_yes_at_price(self):
        order = BuyYes.at_price(market_id=0, price=0.55, quantity=10)
        assert order.market_id == 0
        assert order.limit_price_nanos == 550_000_000
        assert order.quantity == 10

    def test_buy_no_at_price(self):
        order = BuyNo.at_price(market_id=1, price=0.40, quantity=5)
        assert order.market_id == 1
        assert order.limit_price_nanos == 400_000_000
        assert order.quantity == 5

    def test_sell_yes_at_price(self):
        order = SellYes.at_price(market_id=0, price=0.60, quantity=8)
        assert order.market_id == 0
        assert order.limit_price_nanos == 600_000_000
        assert order.quantity == 8

    def test_sell_no_at_price(self):
        order = SellNo.at_price(market_id=2, price=0.35, quantity=3)
        assert order.market_id == 2
        assert order.limit_price_nanos == 350_000_000
        assert order.quantity == 3

    def test_price_clamping(self):
        # Prices should be in valid range after conversion
        order = BuyYes.at_price(market_id=0, price=0.999, quantity=1)
        assert order.limit_price_nanos == 999_000_000

        order = BuyYes.at_price(market_id=0, price=0.001, quantity=1)
        assert order.limit_price_nanos == 1_000_000


class TestNanosConversion:
    """Test nanos/dollars conversions."""

    def test_nanos_per_dollar(self):
        assert NANOS_PER_DOLLAR == 1_000_000_000

    def test_account_balance_conversion(self):
        # $100
        account = Account(id=1, balance_nanos=100 * NANOS_PER_DOLLAR, positions=[])
        assert account.balance_dollars == 100.0

        # $0.50
        account = Account(id=1, balance_nanos=NANOS_PER_DOLLAR // 2, positions=[])
        assert account.balance_dollars == 0.5
