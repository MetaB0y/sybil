"""Tests for sizing strategies (live/strategy.py)."""

import pytest
from live.strategy import KellyStrategy, FlatStrategy, position_orders
from sybil_client import BuyYes, BuyNo, SellYes, SellNo


class TestKellyStrategy:
    def setup_method(self):
        self.s = KellyStrategy()

    def test_bullish_basic(self):
        yes, no = self.s.target(0.70, 0.50, 500.0, 0, 0)
        assert yes > 0 and no == 0

    def test_bearish_basic(self):
        yes, no = self.s.target(0.30, 0.50, 500.0, 0, 0)
        assert yes == 0 and no > 0

    def test_no_edge(self):
        """Edge below min → hold current positions."""
        yes, no = self.s.target(0.51, 0.50, 500.0, 0, 0)
        assert yes == 0 and no == 0

    def test_hold_on_small_edge(self):
        """Edge below min but above exit → hold what you have."""
        yes, no = self.s.target(0.51, 0.50, 500.0, 50, 0)
        assert yes == 50 and no == 0

    def test_exit_on_tiny_edge(self):
        """Edge below exit threshold → close everything."""
        yes, no = self.s.target(0.5001, 0.50, 500.0, 50, 0)
        assert yes == 0 and no == 0

    def test_fractional_kelly(self):
        # edge=0.20, market=0.50 → full kelly = 0.40, 1/3 = 0.133
        yes, _ = self.s.target(0.70, 0.50, 500.0, 0, 0)
        expected = int(0.40 * (1/3) * 500.0 / 0.50)
        assert yes == expected

    def test_max_position_cap(self):
        # Market at 0.10, FV=0.95 → full_kelly=0.944, *1/3=0.315 > cap 0.30
        yes, _ = self.s.target(0.94, 0.10, 1000.0, 0, 0)
        max_shares = int(0.30 * 1000.0 / 0.10)
        assert yes == max_shares

    def test_symmetric(self):
        yes, _ = self.s.target(0.70, 0.50, 500.0, 0, 0)
        _, no = self.s.target(0.30, 0.50, 500.0, 0, 0)
        assert yes == no

    def test_zero_portfolio(self):
        yes, no = self.s.target(0.70, 0.50, 0.0, 0, 0)
        assert yes == 0 and no == 0

    def test_resolved_high_exits(self):
        """Market at 0.99 → resolved, exit all positions."""
        yes, no = self.s.target(0.15, 0.99, 500.0, 100, 0)
        assert yes == 0 and no == 0

    def test_resolved_low_exits(self):
        """Market at 0.01 → resolved, exit all positions."""
        yes, no = self.s.target(0.85, 0.01, 500.0, 0, 100)
        assert yes == 0 and no == 0

    def test_resolved_no_new_position(self):
        """Don't open positions on resolved markets even with huge edge."""
        yes, no = self.s.target(0.50, 0.99, 500.0, 0, 0)
        assert yes == 0 and no == 0

    def test_name(self):
        assert self.s.name == "kelly"


class TestFlatStrategy:
    def setup_method(self):
        self.s = FlatStrategy(bet_dollars=20.0, min_edge=0.03)

    def test_bullish_flat_bet(self):
        yes, no = self.s.target(0.60, 0.50, 500.0, 0, 0)
        # $20 / $0.50 = 40 shares
        assert yes == 40 and no == 0

    def test_bearish_flat_bet(self):
        yes, no = self.s.target(0.40, 0.50, 500.0, 0, 0)
        assert yes == 0 and no == 40

    def test_no_edge(self):
        yes, no = self.s.target(0.51, 0.50, 500.0, 0, 0)
        assert yes == 0 and no == 0

    def test_dont_add_to_existing(self):
        """Flat strategy doesn't add to existing correct-side positions."""
        yes, no = self.s.target(0.60, 0.50, 500.0, 40, 0)
        assert yes == 40 and no == 0

    def test_exit_on_edge_flip(self):
        """If edge flips against position, exit completely."""
        yes, no = self.s.target(0.40, 0.50, 500.0, 40, 0)
        assert yes == 0 and no == 0

    def test_hard_exit_rule(self):
        """Market moves strongly against → sell at least half."""
        # Long YES but market at 25% (below 30% threshold)
        yes, no = self.s.target(0.60, 0.25, 500.0, 100, 0)
        assert yes == 50  # half of 100
        assert no == 0

    def test_hard_exit_rule_no(self):
        """Long NO but market at 75% → sell at least half."""
        yes, no = self.s.target(0.40, 0.75, 500.0, 0, 100)
        assert yes == 0
        assert no == 50  # half of 100

    def test_resolved_high_exits(self):
        """Market at 0.99 → resolved, exit everything."""
        yes, no = self.s.target(0.40, 0.99, 500.0, 0, 40)
        assert yes == 0 and no == 0

    def test_resolved_low_exits(self):
        """Market at 0.01 → resolved, exit everything."""
        yes, no = self.s.target(0.60, 0.01, 500.0, 40, 0)
        assert yes == 0 and no == 0

    def test_name(self):
        assert self.s.name == "flat"

    def test_longer_rebalance(self):
        assert self.s.rebalance_interval_s > KellyStrategy().rebalance_interval_s


class TestPositionOrders:
    def test_buy_yes_from_zero(self):
        orders = position_orders(1, 100, 0, 0, 0, 0.70, 0.50, 200.0)
        assert len(orders) == 1
        assert isinstance(orders[0], BuyYes)
        assert orders[0].quantity == 100

    def test_exit_wrong_side(self):
        orders = position_orders(1, 50, 0, 0, 30, 0.70, 0.50, 100.0)
        sell_nos = [o for o in orders if isinstance(o, SellNo)]
        buy_yeses = [o for o in orders if isinstance(o, BuyYes)]
        assert len(sell_nos) == 1 and sell_nos[0].quantity == 30
        assert len(buy_yeses) == 1

    def test_trim_oversized(self):
        orders = position_orders(1, 50, 0, 100, 0, 0.70, 0.50, 0.0)
        assert len(orders) == 1
        assert isinstance(orders[0], SellYes)
        assert orders[0].quantity == 50

    def test_full_exit(self):
        orders = position_orders(1, 0, 0, 100, 50, 0.50, 0.50, 0.0)
        assert len([o for o in orders if isinstance(o, SellYes)]) == 1
        assert len([o for o in orders if isinstance(o, SellNo)]) == 1

    def test_cash_limited_buy(self):
        orders = position_orders(1, 100, 0, 0, 0, 0.70, 0.50, 25.0)
        assert len(orders) == 1
        assert orders[0].quantity == 50  # $25 / $0.50

    def test_already_at_target(self):
        orders = position_orders(1, 100, 0, 100, 0, 0.70, 0.50, 100.0)
        assert len(orders) == 0

    def test_sell_uses_market_price(self):
        orders = position_orders(1, 0, 0, 50, 0, 0.70, 0.50, 0.0)
        from sybil_client.types import NANOS_PER_DOLLAR
        assert orders[0].limit_price_nanos == int(0.50 * NANOS_PER_DOLLAR)

    def test_buy_uses_fair_value(self):
        orders = position_orders(1, 100, 0, 0, 0, 0.70, 0.50, 200.0)
        from sybil_client.types import NANOS_PER_DOLLAR
        assert orders[0].limit_price_nanos == int(0.70 * NANOS_PER_DOLLAR)
