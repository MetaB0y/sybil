"""Pluggable sizing strategies for LLM traders.

Two philosophies compete:
- KellyStrategy: size proportional to edge (1/3 Kelly). Fewer positions, bigger bets.
- FlatStrategy: fixed $ per bet, hard exit rules. Many small positions, terminator2-style.

Both take a fair_value + market_price and return target positions.
"""

from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from dataclasses import dataclass

from sybil_client import BuyNo, BuyYes, OrderSpec, SellNo, SellYes

log = logging.getLogger(__name__)

# Markets at or beyond these prices are effectively resolved — exit all positions.
RESOLVED_HIGH = 0.95
RESOLVED_LOW = 0.05


# --------------------------------------------------------------------------- #
# Base
# --------------------------------------------------------------------------- #
class SizingStrategy(ABC):
    """Given a fair value and market price, compute target positions."""

    @abstractmethod
    def target(
        self,
        fair_value: float,
        market_price: float,
        portfolio_value: float,
        current_yes: int,
        current_no: int,
    ) -> tuple[int, int]:
        """Return (target_yes, target_no). Exactly one should be > 0, or both 0."""
        ...

    @property
    @abstractmethod
    def name(self) -> str: ...

    @property
    def rebalance_interval_s(self) -> float:
        """How often to run position management (seconds)."""
        return 30.0


# --------------------------------------------------------------------------- #
# Kelly strategy — size proportional to edge
# --------------------------------------------------------------------------- #
class KellyStrategy(SizingStrategy):
    """Fractional Kelly criterion. Continuous rebalancing.

    Pros: capital-efficient, mathematically optimal for growth.
    Cons: can build large positions, sensitive to FV accuracy.
    """

    def __init__(
        self,
        kelly_fraction: float = 1 / 3,
        min_edge: float = 0.02,
        exit_edge: float = 0.005,
        max_position_frac: float = 0.30,
    ):
        self.kelly_fraction = kelly_fraction
        self.min_edge = min_edge
        self.exit_edge = exit_edge
        self.max_position_frac = max_position_frac

    @property
    def name(self) -> str:
        return "kelly"

    def target(
        self,
        fair_value: float,
        market_price: float,
        portfolio_value: float,
        current_yes: int,
        current_no: int,
    ) -> tuple[int, int]:
        # Resolved market → exit everything
        if market_price >= RESOLVED_HIGH or market_price <= RESOLVED_LOW:
            return (0, 0)

        edge = fair_value - market_price

        # Below exit threshold → close everything
        if abs(edge) < self.exit_edge:
            return (0, 0)

        # Below min edge → hold current, don't open new
        if abs(edge) < self.min_edge:
            return (current_yes, current_no)

        if edge > 0:
            full_kelly = edge / (1 - market_price)
            bet_value = full_kelly * self.kelly_fraction * portfolio_value
            bet_value = min(bet_value, self.max_position_frac * portfolio_value)
            target_yes = int(bet_value / market_price)
            return (max(target_yes, 0), 0)
        else:
            full_kelly = abs(edge) / market_price
            bet_value = full_kelly * self.kelly_fraction * portfolio_value
            bet_value = min(bet_value, self.max_position_frac * portfolio_value)
            no_price = 1 - market_price
            target_no = int(bet_value / no_price)
            return (0, max(target_no, 0))


# --------------------------------------------------------------------------- #
# Flat strategy — fixed bet size, hard exit rules (terminator2-inspired)
# --------------------------------------------------------------------------- #
class FlatStrategy(SizingStrategy):
    """Fixed $ per bet with hard mechanical exit rules.

    Inspired by terminator2 (profitable on Manifold):
    - Flat $20 bets regardless of edge magnitude
    - Hard exit: if market moves 70% against position, sell at least half
    - Don't add to existing positions
    - Diversify across many markets

    Pros: simple, robust to FV errors, prevents conviction loops.
    Cons: doesn't scale with confidence, leaves edge on the table.
    """

    def __init__(
        self,
        bet_dollars: float = 20.0,
        min_edge: float = 0.03,
        exit_against_pct: float = 0.70,
        exit_fraction: float = 0.5,
    ):
        self.bet_dollars = bet_dollars
        self.min_edge = min_edge
        # If price has moved this far against our direction, exit
        self.exit_against_pct = exit_against_pct
        self.exit_fraction = exit_fraction

    @property
    def name(self) -> str:
        return "flat"

    @property
    def rebalance_interval_s(self) -> float:
        # Flat strategy doesn't need frequent rebalancing
        return 60.0

    def target(
        self,
        fair_value: float,
        market_price: float,
        portfolio_value: float,
        current_yes: int,
        current_no: int,
    ) -> tuple[int, int]:
        # Resolved market → exit everything
        if market_price >= RESOLVED_HIGH or market_price <= RESOLVED_LOW:
            return (0, 0)

        edge = fair_value - market_price

        # Hard exit rule: market has moved strongly against our position
        if current_yes > 0 and market_price < (1 - self.exit_against_pct):
            # We're long YES but market says <30% → exit at least half
            keep = int(current_yes * (1 - self.exit_fraction))
            return (keep, 0)

        if current_no > 0 and market_price > self.exit_against_pct:
            # We're long NO but market says >70% → exit at least half
            keep = int(current_no * (1 - self.exit_fraction))
            return (0, keep)

        # If edge flipped against our position, exit completely
        if current_yes > 0 and edge < -self.min_edge:
            return (0, 0)
        if current_no > 0 and edge > self.min_edge:
            return (0, 0)

        # Not enough edge → hold what we have
        if abs(edge) < self.min_edge:
            return (current_yes, current_no)

        # Already have a position in the right direction → hold, don't add
        if edge > 0 and current_yes > 0:
            return (current_yes, 0)
        if edge < 0 and current_no > 0:
            return (0, current_no)

        # New position: flat $bet_dollars
        if edge > 0:
            qty = int(self.bet_dollars / market_price) if market_price > 0 else 0
            return (max(qty, 0), 0)
        else:
            no_price = 1 - market_price
            qty = int(self.bet_dollars / no_price) if no_price > 0 else 0
            return (0, max(qty, 0))


# --------------------------------------------------------------------------- #
# Order generation (shared by all strategies)
# --------------------------------------------------------------------------- #
def position_orders(
    market_id: int,
    target_yes: int,
    target_no: int,
    current_yes: int,
    current_no: int,
    fair_value: float,
    market_price: float,
    available_cash: float,
) -> list[OrderSpec]:
    """Generate orders to move from current to target positions.

    Sells use market_price (willing to exit at current price).
    Buys use fair_value as limit (willing to pay up to FV).
    """
    orders: list[OrderSpec] = []

    # Exit wrong-side positions first (frees cash)
    if target_yes == 0 and current_yes > 0:
        orders.append(SellYes.at_price(market_id, market_price, current_yes))
    if target_no == 0 and current_no > 0:
        orders.append(SellNo.at_price(market_id, 1 - market_price, current_no))

    # Trim oversized positions
    if target_yes > 0 and current_yes > target_yes:
        excess = current_yes - target_yes
        orders.append(SellYes.at_price(market_id, market_price, excess))
    if target_no > 0 and current_no > target_no:
        excess = current_no - target_no
        orders.append(SellNo.at_price(market_id, 1 - market_price, excess))

    # Scale into target (buy)
    if target_yes > current_yes:
        deficit = target_yes - current_yes
        cost_per_share = fair_value
        affordable = int(available_cash / cost_per_share) if cost_per_share > 0 else 0
        qty = min(deficit, affordable)
        if qty > 0:
            orders.append(BuyYes.at_price(market_id, fair_value, qty))

    if target_no > current_no:
        deficit = target_no - current_no
        no_limit = 1 - fair_value
        cost_per_share = no_limit
        affordable = int(available_cash / cost_per_share) if cost_per_share > 0 else 0
        qty = min(deficit, affordable)
        if qty > 0:
            orders.append(BuyNo.at_price(market_id, no_limit, qty))

    return orders
