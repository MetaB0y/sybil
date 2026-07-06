"""Pluggable sizing strategies for LLM traders.

Two philosophies compete:
- KellyStrategy: size proportional to edge (1/3 Kelly). Fewer positions, bigger bets.
- FlatStrategy: fixed $ per bet, hard exit rules. Many small positions, terminator2-style.

Both take a fair_value + market_price and return target positions.
"""

from __future__ import annotations

import logging
import math
from abc import ABC, abstractmethod
from dataclasses import dataclass

from sybil_client import BuyNo, BuyYes, OrderSpec, SellNo, SellYes

log = logging.getLogger(__name__)

# Markets at or beyond these prices are effectively resolved — exit all positions.
RESOLVED_HIGH = 0.95
RESOLVED_LOW = 0.05


@dataclass(frozen=True)
class FairValueFreshnessConfig:
    """Controls how stale analyst fair values are dampened by the sizer."""

    ttl_s: float = 10 * 60
    half_life_s: float = 30 * 60
    hard_expiry_s: float = 2 * 60 * 60

    def __post_init__(self) -> None:
        if self.ttl_s < 0:
            raise ValueError("ttl_s must be non-negative")
        if self.half_life_s <= 0:
            raise ValueError("half_life_s must be positive")
        if self.hard_expiry_s < self.ttl_s:
            raise ValueError("hard_expiry_s must be >= ttl_s")


@dataclass(frozen=True)
class FreshFairValue:
    """A raw FV after freshness decay has been applied."""

    raw_fair_value: float
    effective_fair_value: float | None
    age_s: float
    freshness_factor: float

    @property
    def expired(self) -> bool:
        return self.effective_fair_value is None


def _clamp01(value: float) -> float:
    if math.isnan(value):
        return 0.0
    return min(1.0, max(0.0, value))


def effective_fair_value(
    raw_fair_value: float,
    market_price: float,
    age_s: float,
    config: FairValueFreshnessConfig | None = None,
) -> FreshFairValue:
    """Decay stale FV toward market price, or expire it entirely.

    Fresh estimates are used as-is until ``ttl_s``. After that, the raw edge
    decays exponentially toward zero with the configured half-life. Once the
    hard expiry is reached the caller should behave as though no FV exists.
    """
    cfg = config or FairValueFreshnessConfig()
    age = max(0.0, age_s)
    raw = _clamp01(raw_fair_value)
    market = _clamp01(market_price)

    if age >= cfg.hard_expiry_s:
        return FreshFairValue(raw, None, age, 0.0)
    if age <= cfg.ttl_s:
        return FreshFairValue(raw, raw, age, 1.0)

    decay_elapsed = age - cfg.ttl_s
    freshness = 0.5 ** (decay_elapsed / cfg.half_life_s)
    effective = market + (raw - market) * freshness
    return FreshFairValue(raw, _clamp01(effective), age, freshness)


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
        market_id: int | None = None,
        confidence: float | None = None,
        freshness_factor: float = 1.0,
    ) -> tuple[int, int]:
        """Return (target_yes, target_no). Exactly one should be > 0, or both 0.

        ``market_id`` lets stateful strategies key per-position bookkeeping
        (e.g. entry prices). Stateless strategies ignore it.
        """
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
        market_id: int | None = None,
        confidence: float | None = None,
        freshness_factor: float = 1.0,
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

        shrink = _clamp01(freshness_factor)
        if confidence is not None:
            shrink *= _clamp01(confidence)
        shrink = min(1.0, shrink)

        if edge > 0:
            full_kelly = edge / (1 - market_price)
            bet_value = full_kelly * self.kelly_fraction * shrink * portfolio_value
            bet_value = min(bet_value, self.max_position_frac * portfolio_value)
            target_yes = int(bet_value / market_price)
            return (max(target_yes, 0), 0)
        else:
            full_kelly = abs(edge) / market_price
            bet_value = full_kelly * self.kelly_fraction * shrink * portfolio_value
            bet_value = min(bet_value, self.max_position_frac * portfolio_value)
            no_price = 1 - market_price
            target_no = int(bet_value / no_price)
            return (0, max(target_no, 0))


# --------------------------------------------------------------------------- #
# Flat strategy — fixed bet size, hard exit rules (terminator2-inspired)
# --------------------------------------------------------------------------- #
class FlatStrategy(SizingStrategy):
    """Fixed $ per bet with a cost-basis-relative hard exit.

    Inspired by terminator2 (profitable on Manifold):
    - Flat $20 bets regardless of edge magnitude
    - Don't add to existing positions; diversify across many markets

    Exit rule (AR-4)
    ----------------
    The hard exit keys on adverse movement **relative to the entry price**, not
    on the absolute price level. The strategy records the price at which each
    position was first observed and exits fully when the position's mark has
    lost ``exit_adverse_frac`` of its value versus that entry:

    - long YES bought at ``entry`` → exit when ``market_price`` has fallen so
      that ``(entry - market_price) / entry >= exit_adverse_frac``;
    - long NO (mark ``1 - market_price``) → exit when it has fallen so that
      ``(market_price - entry) / (1 - entry) >= exit_adverse_frac``.

    Keying on the absolute level (the old ``price < 0.30`` / ``> 0.70`` rule)
    meant any position on a legitimately cheap/expensive market was force-sold
    every rebalance and immediately re-bought, an endless buy/sell churn. Tying
    the exit to entry makes it a one-shot stop-loss and re-entry re-arms it at
    the new basis.

    Pros: simple, robust to FV errors, prevents conviction loops.
    Cons: doesn't scale with confidence, leaves edge on the table.
    """

    def __init__(
        self,
        bet_dollars: float = 20.0,
        min_edge: float = 0.03,
        exit_adverse_frac: float = 0.30,
    ):
        self.bet_dollars = bet_dollars
        self.min_edge = min_edge
        # Exit once a position has lost this fraction of its value vs entry.
        self.exit_adverse_frac = exit_adverse_frac
        # market_id -> entry price (Polymarket-style YES price at first sighting)
        self._entry_prices: dict[int | None, float] = {}

    @property
    def name(self) -> str:
        return "flat"

    @property
    def rebalance_interval_s(self) -> float:
        # Flat strategy doesn't need frequent rebalancing
        return 60.0

    def _forget_entry(self, market_id: int | None) -> None:
        self._entry_prices.pop(market_id, None)

    def target(
        self,
        fair_value: float,
        market_price: float,
        portfolio_value: float,
        current_yes: int,
        current_no: int,
        market_id: int | None = None,
        confidence: float | None = None,
        freshness_factor: float = 1.0,
    ) -> tuple[int, int]:
        del confidence, freshness_factor
        # Resolved market → exit everything
        if market_price >= RESOLVED_HIGH or market_price <= RESOLVED_LOW:
            self._forget_entry(market_id)
            return (0, 0)

        edge = fair_value - market_price

        # Track the entry price: record it the first block we observe a holding,
        # and forget it once we are flat so a fresh position re-arms the stop.
        if current_yes > 0 or current_no > 0:
            entry = self._entry_prices.setdefault(market_id, market_price)
        else:
            self._forget_entry(market_id)
            entry = None

        # Hard exit: the position has moved adversely vs its entry price.
        if current_yes > 0 and entry is not None and entry > 0:
            if (entry - market_price) / entry >= self.exit_adverse_frac:
                self._forget_entry(market_id)
                return (0, 0)
        if current_no > 0 and entry is not None and entry < 1:
            if (market_price - entry) / (1 - entry) >= self.exit_adverse_frac:
                self._forget_entry(market_id)
                return (0, 0)

        # If edge flipped against our position, exit completely
        if current_yes > 0 and edge < -self.min_edge:
            self._forget_entry(market_id)
            return (0, 0)
        if current_no > 0 and edge > self.min_edge:
            self._forget_entry(market_id)
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
