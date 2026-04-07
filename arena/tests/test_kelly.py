"""Tests for Kelly-based position sizing (live/trader.py)."""

import pytest
from live.trader import kelly_target, position_orders, KELLY_FRACTION, MIN_EDGE, EXIT_EDGE
from sybil_client import BuyYes, BuyNo, SellYes, SellNo


class TestKellyTarget:
    def test_bullish_basic(self):
        """FV > market → buy YES."""
        target_yes, target_no = kelly_target(0.70, 0.50, 500.0)
        assert target_yes > 0
        assert target_no == 0

    def test_bearish_basic(self):
        """FV < market → buy NO."""
        target_yes, target_no = kelly_target(0.30, 0.50, 500.0)
        assert target_yes == 0
        assert target_no > 0

    def test_no_edge(self):
        """Edge below MIN_EDGE → no position."""
        target_yes, target_no = kelly_target(0.51, 0.50, 500.0)
        assert target_yes == 0
        assert target_no == 0

    def test_edge_exactly_at_threshold(self):
        """Edge exactly at MIN_EDGE → trades."""
        fv = 0.50 + MIN_EDGE
        target_yes, target_no = kelly_target(fv, 0.50, 500.0)
        assert target_yes > 0

    def test_fractional_kelly(self):
        """Verify position is 1/3 of full Kelly."""
        # edge=0.20, market=0.50 → full kelly = 0.20/(1-0.50) = 0.40
        # 1/3 kelly = 0.133, bet = 0.133 * 500 = 66.67, shares = 66.67/0.50 = 133
        target_yes, _ = kelly_target(0.70, 0.50, 500.0)
        full_kelly_shares = int(0.40 * 500.0 / 0.50)  # 400
        third_kelly_shares = int(0.40 * KELLY_FRACTION * 500.0 / 0.50)  # 133
        assert target_yes == third_kelly_shares

    def test_max_position_cap(self):
        """Very large edge gets capped at MAX_POSITION_FRAC."""
        # FV=0.99, market=0.01 → huge Kelly, but capped at 30%
        target_yes, _ = kelly_target(0.99, 0.01, 1000.0)
        max_value = 0.30 * 1000.0  # $300
        max_shares = int(max_value / 0.01)  # 30000
        assert target_yes == max_shares

    def test_symmetric_bullish_bearish(self):
        """Symmetric edges give symmetric positions."""
        yes, _ = kelly_target(0.70, 0.50, 500.0)
        _, no = kelly_target(0.30, 0.50, 500.0)
        assert yes == no  # symmetric around 0.50

    def test_zero_portfolio(self):
        """Zero portfolio → no position."""
        target_yes, target_no = kelly_target(0.70, 0.50, 0.0)
        assert target_yes == 0
        assert target_no == 0


class TestPositionOrders:
    def test_buy_yes_from_zero(self):
        """Start from zero, target YES position."""
        orders = position_orders(
            market_id=1, target_yes=100, target_no=0,
            current_yes=0, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=200.0,
        )
        assert len(orders) == 1
        assert isinstance(orders[0], BuyYes)
        assert orders[0].quantity == 100

    def test_buy_no_from_zero(self):
        """Start from zero, target NO position."""
        orders = position_orders(
            market_id=1, target_yes=0, target_no=100,
            current_yes=0, current_no=0,
            fair_value=0.30, market_price=0.50,
            available_cash=200.0,
        )
        assert len(orders) == 1
        assert isinstance(orders[0], BuyNo)

    def test_exit_wrong_side(self):
        """Holding NO, target is YES → sell NO first, then buy YES."""
        orders = position_orders(
            market_id=1, target_yes=50, target_no=0,
            current_yes=0, current_no=30,
            fair_value=0.70, market_price=0.50,
            available_cash=100.0,
        )
        # Should sell NO (exit wrong side) + buy YES
        sell_nos = [o for o in orders if isinstance(o, SellNo)]
        buy_yeses = [o for o in orders if isinstance(o, BuyYes)]
        assert len(sell_nos) == 1
        assert sell_nos[0].quantity == 30
        assert len(buy_yeses) == 1

    def test_trim_oversized(self):
        """Holding more YES than target → sell excess."""
        orders = position_orders(
            market_id=1, target_yes=50, target_no=0,
            current_yes=100, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=0.0,
        )
        assert len(orders) == 1
        assert isinstance(orders[0], SellYes)
        assert orders[0].quantity == 50

    def test_full_exit(self):
        """Target is zero → sell everything."""
        orders = position_orders(
            market_id=1, target_yes=0, target_no=0,
            current_yes=100, current_no=50,
            fair_value=0.50, market_price=0.50,
            available_cash=0.0,
        )
        sell_yes = [o for o in orders if isinstance(o, SellYes)]
        sell_no = [o for o in orders if isinstance(o, SellNo)]
        assert len(sell_yes) == 1 and sell_yes[0].quantity == 100
        assert len(sell_no) == 1 and sell_no[0].quantity == 50

    def test_cash_limited_buy(self):
        """Not enough cash for full target → buy what we can afford."""
        orders = position_orders(
            market_id=1, target_yes=100, target_no=0,
            current_yes=0, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=25.0,  # can only afford 50 shares at $0.50
        )
        assert len(orders) == 1
        assert isinstance(orders[0], BuyYes)
        assert orders[0].quantity == 50

    def test_already_at_target(self):
        """Already at target → no orders."""
        orders = position_orders(
            market_id=1, target_yes=100, target_no=0,
            current_yes=100, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=100.0,
        )
        assert len(orders) == 0

    def test_sell_uses_market_price(self):
        """Sells should use market price, not fair value."""
        orders = position_orders(
            market_id=1, target_yes=0, target_no=0,
            current_yes=50, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=0.0,
        )
        assert len(orders) == 1
        assert isinstance(orders[0], SellYes)
        # Limit price should be at market price (0.50)
        from sybil_client.types import NANOS_PER_DOLLAR
        assert orders[0].limit_price_nanos == int(0.50 * NANOS_PER_DOLLAR)

    def test_buy_uses_fair_value(self):
        """Buys should use fair value as limit price."""
        orders = position_orders(
            market_id=1, target_yes=100, target_no=0,
            current_yes=0, current_no=0,
            fair_value=0.70, market_price=0.50,
            available_cash=200.0,
        )
        assert len(orders) == 1
        from sybil_client.types import NANOS_PER_DOLLAR
        assert orders[0].limit_price_nanos == int(0.70 * NANOS_PER_DOLLAR)
