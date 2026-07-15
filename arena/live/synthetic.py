"""Deterministic synthetic traders for live mirror/native markets.

The mirror market maker lives in ``sybil-polymarket``. These arena strategies
are the lightweight taker flow around it:

- reference-backed fast traders move Sybil prices toward Polymarket;
- native noise traders perturb local prices around the previous Sybil batch.
"""

from __future__ import annotations

import math
import random
import time
from dataclasses import dataclass, replace
from typing import Literal

from bots.base import BaseAgent
from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes
from sybil_client.types import NANOS_PER_DOLLAR, Market

MIN_PRICE = 0.01
MAX_PRICE = 0.99
MAX_RANDOMIZATION_RANGE = 0.02

Outcome = Literal["YES", "NO"]


@dataclass(frozen=True)
class SyntheticStrategyConfig:
    """Shared config read by fast-reference and native-noise strategies."""

    max_inventory: int = 50
    quote_width: float = 0.005
    notional_budget: float = 5.0
    random_seed: int = 42
    randomization_range: float = MAX_RANDOMIZATION_RANGE
    enabled_market_ids: frozenset[int] | None = None
    # SYB "zero-fills" fix: aggressive two-sided crossing noise. When enabled,
    # noise traders post BOTH a BuyYes and a BuyNo per market at prices whose
    # sum exceeds $1, so they cross via complete-set minting (p+q>=1) against
    # the resting book, against other noise accounts, or (with GTC) over time.
    crossing_enabled: bool = True
    crossing_edge: float = 0.03  # how far past mid each side crosses; sum = 1 + 2*edge
    crossing_markets_per_block: int = 6  # 0 = every eligible market each block

    def __post_init__(self) -> None:
        if self.max_inventory < 0:
            raise ValueError("max_inventory must be non-negative")
        if self.quote_width < 0:
            raise ValueError("quote_width must be non-negative")
        if self.notional_budget < 0:
            raise ValueError("notional_budget must be non-negative")
        if self.randomization_range < 0:
            raise ValueError("randomization_range must be non-negative")
        if self.crossing_edge < 0:
            raise ValueError("crossing_edge must be non-negative")
        if self.crossing_markets_per_block < 0:
            raise ValueError("crossing_markets_per_block must be non-negative")
        if self.enabled_market_ids is not None and not isinstance(
            self.enabled_market_ids, frozenset
        ):
            object.__setattr__(self, "enabled_market_ids", frozenset(self.enabled_market_ids))

    @property
    def bounded_randomization_range(self) -> float:
        return min(self.randomization_range, MAX_RANDOMIZATION_RANGE)

    def enabled(self, market_id: int) -> bool:
        return self.enabled_market_ids is None or market_id in self.enabled_market_ids

    def with_seed(self, seed: int) -> "SyntheticStrategyConfig":
        return replace(self, random_seed=seed)


def has_reference_price(market: Market) -> bool:
    ref = getattr(market, "reference_price_nanos", None)
    expires_at_ms = getattr(market, "reference_price_expires_at_ms", None)
    return (
        ref is not None
        and ref > 0
        and (expires_at_ms is None or int(time.time() * 1000) <= expires_at_ms)
    )


def is_mirror_market(market: Market) -> bool:
    """Return whether a market is mirror-originated for synthetic routing."""
    if has_reference_price(market):
        return True
    if getattr(market, "polymarket_condition_id", None):
        return True
    tags = {str(tag).strip().lower().replace("-", " ") for tag in getattr(market, "tags", [])}
    return "polymarket" in tags


def _clamp_price(price: float) -> float:
    return min(MAX_PRICE, max(MIN_PRICE, price))


def _previous_sybil_price(block: Block, market: Market) -> float | None:
    if market.id in block.clearing_prices:
        yes_nanos, _ = block.clearing_prices[market.id]
        return _clamp_price(yes_nanos / NANOS_PER_DOLLAR)
    if market.yes_price_nanos > 0:
        return _clamp_price(market.yes_price)
    return None


def _reference_price(market: Market) -> float | None:
    if not has_reference_price(market):
        return None
    return _clamp_price(market.reference_price_nanos / NANOS_PER_DOLLAR)


def _positions(
    positions: dict[tuple[int, str], float],
    market_id: int,
) -> tuple[float, float]:
    return (
        max(0.0, float(positions.get((market_id, "YES"), 0))),
        max(0.0, float(positions.get((market_id, "NO"), 0))),
    )


def _buy_qty(budget: float, price: float, remaining_inventory: float, cash: float) -> int:
    affordable_dollars = min(budget, cash)
    if affordable_dollars <= 0 or price <= 0 or remaining_inventory <= 0:
        return 0
    return max(0, math.floor(min(remaining_inventory, affordable_dollars / price)))


def _sell_qty(budget: float, price: float, held: float) -> int:
    if held <= 0:
        return 0
    if budget <= 0 or price <= 0:
        return math.floor(held)
    return max(0, math.floor(min(held, budget / price)))


class _InventoryAwareStrategy:
    def __init__(self, config: SyntheticStrategyConfig):
        self.config = config
        self.rng = random.Random(config.random_seed)

    def _eligible_markets(self, markets: dict[int, Market]) -> list[Market]:
        raise NotImplementedError

    def _target_price(self, block: Block, market: Market) -> float | None:
        raise NotImplementedError

    def generate_orders(
        self,
        block: Block,
        markets: dict[int, Market],
        positions: dict[tuple[int, str], float],
        cash: float,
    ) -> list[OrderSpec]:
        candidates = self._eligible_markets(markets)
        if not candidates:
            return []

        market = self.rng.choice(candidates)
        current = _previous_sybil_price(block, market)
        target = self._target_price(block, market)
        if current is None or target is None:
            return []

        yes_pos, no_pos = _positions(positions, market.id)
        return self._directional_order(market.id, current, target, yes_pos, no_pos, cash)

    def _directional_order(
        self,
        market_id: int,
        current: float,
        target: float,
        yes_pos: float,
        no_pos: float,
        cash: float,
    ) -> list[OrderSpec]:
        max_inventory = self.config.max_inventory
        if max_inventory == 0:
            return []

        net = yes_pos - no_pos
        if net > max_inventory and yes_pos > 0:
            return self._sell_yes(market_id, current, yes_pos - max_inventory)
        if net < -max_inventory and no_pos > 0:
            return self._sell_no(market_id, current, no_pos - max_inventory)

        if target > current + self.config.quote_width:
            return self._move_yes(market_id, target, current, yes_pos, no_pos, cash)
        if target < current - self.config.quote_width:
            return self._move_no(market_id, target, current, yes_pos, no_pos, cash)
        return []

    def _move_yes(
        self,
        market_id: int,
        target: float,
        current: float,
        yes_pos: float,
        no_pos: float,
        cash: float,
    ) -> list[OrderSpec]:
        if no_pos > 0:
            return self._sell_no(market_id, current, no_pos)

        price = _clamp_price(target)
        qty = _buy_qty(
            self.config.notional_budget,
            price,
            self.config.max_inventory - yes_pos,
            cash,
        )
        if qty <= 0:
            return []
        return [BuyYes.at_price(market_id, price, qty)]

    def _move_no(
        self,
        market_id: int,
        target: float,
        current: float,
        yes_pos: float,
        no_pos: float,
        cash: float,
    ) -> list[OrderSpec]:
        if yes_pos > 0:
            return self._sell_yes(market_id, current, yes_pos)

        no_price = _clamp_price(1.0 - target)
        qty = _buy_qty(
            self.config.notional_budget,
            no_price,
            self.config.max_inventory - no_pos,
            cash,
        )
        if qty <= 0:
            return []
        return [BuyNo.at_price(market_id, no_price, qty)]

    def _sell_yes(self, market_id: int, current: float, held: float) -> list[OrderSpec]:
        price = _clamp_price(current - self.config.quote_width)
        qty = _sell_qty(self.config.notional_budget, price, held)
        if qty <= 0:
            return []
        return [SellYes.at_price(market_id, price, qty)]

    def _sell_no(self, market_id: int, current: float, held: float) -> list[OrderSpec]:
        price = _clamp_price((1.0 - current) - self.config.quote_width)
        qty = _sell_qty(self.config.notional_budget, price, held)
        if qty <= 0:
            return []
        return [SellNo.at_price(market_id, price, qty)]


class FastReferenceStrategy(_InventoryAwareStrategy):
    """Trade mirror markets toward their external reference price."""

    def _eligible_markets(self, markets: dict[int, Market]) -> list[Market]:
        return sorted(
            (
                market
                for market in markets.values()
                if self.config.enabled(market.id) and has_reference_price(market)
            ),
            key=lambda market: market.id,
        )

    def _target_price(self, block: Block, market: Market) -> float | None:
        del block
        ref = _reference_price(market)
        if ref is None:
            return None
        jitter = self.rng.uniform(
            -self.config.bounded_randomization_range,
            self.config.bounded_randomization_range,
        )
        return _clamp_price(ref + jitter)


class NativeNoiseStrategy(_InventoryAwareStrategy):
    """Random native-market flow around the previous Sybil batch price."""

    def _eligible_markets(self, markets: dict[int, Market]) -> list[Market]:
        return sorted(
            (
                market
                for market in markets.values()
                if self.config.enabled(market.id) and not is_mirror_market(market)
            ),
            key=lambda market: market.id,
        )

    def _target_price(self, block: Block, market: Market) -> float | None:
        current = _previous_sybil_price(block, market)
        if current is None:
            return None
        direction = -1.0 if self.rng.random() < 0.5 else 1.0
        delta = self.rng.uniform(0.0, self.config.bounded_randomization_range)
        return _clamp_price(current + direction * delta)


class FastReferenceTrader(BaseAgent):
    """BaseAgent adapter for :class:`FastReferenceStrategy`."""

    def __init__(
        self,
        client,
        account_id: int,
        *,
        markets_info: dict[int, Market],
        config: SyntheticStrategyConfig | None = None,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name or "FastReferenceTrader", market_ids)
        self.markets_info = markets_info
        self.strategy = FastReferenceStrategy(config or SyntheticStrategyConfig())

    async def on_block(self, block: Block) -> list[OrderSpec]:
        markets = self.markets_info
        if self.market_ids is not None:
            markets = {
                market_id: market
                for market_id, market in markets.items()
                if market_id in self.market_ids
            }
        return self.strategy.generate_orders(
            block, markets, self.positions, self.current_balance
        )


class CrossingNoiseStrategy:
    """Aggressive two-sided taker that reliably produces complete-set matches.

    The zero-fills problem is order-flow density: LLM bots + MM post one-sided,
    non-crossing IOC quotes, so nothing crosses in-batch and no durable book
    forms. This strategy fixes that directly. Each block it picks up to
    ``crossing_markets_per_block`` markets from the runner's selected cohort
    and, on each, submits BOTH a BuyYes and a BuyNo at prices whose
    sum exceeds $1. Because BuyYes@p + BuyNo@q with p+q>=1 mints a complete set,
    these orders cross — against the resting book, against the opposite side of
    other well-funded noise accounts, or (under GTC) accumulated over time.
    Well-funded accounts absorb the small (~2*edge) per-set mint premium.
    """

    def __init__(self, config: SyntheticStrategyConfig):
        self.config = config
        self.rng = random.Random(config.random_seed)

    def _eligible_markets(self, markets: dict[int, Market]) -> list[Market]:
        return sorted(
            (market for market in markets.values() if self.config.enabled(market.id)),
            key=lambda market: market.id,
        )

    def generate_orders(
        self,
        block: Block,
        markets: dict[int, Market],
        positions: dict[tuple[int, str], float],
        cash: float,
    ) -> list[OrderSpec]:
        candidates = self._eligible_markets(markets)
        if not candidates:
            return []

        per_block = self.config.crossing_markets_per_block
        if per_block and per_block < len(candidates):
            chosen = self.rng.sample(candidates, per_block)
        else:
            chosen = candidates

        edge = self.config.crossing_edge
        budget = self.config.notional_budget
        remaining_cash = cash
        orders: list[OrderSpec] = []
        for market in chosen:
            if remaining_cash <= 0:
                break
            # Anchor on the previous Sybil price; fresh markets default to 0.5.
            mid = _previous_sybil_price(block, market)
            if mid is None:
                mid = 0.5
            # Small jitter so prices differ across accounts and over time.
            jitter = self.rng.uniform(
                -self.config.bounded_randomization_range,
                self.config.bounded_randomization_range,
            )
            mid = _clamp_price(mid + jitter)

            yes_pos, no_pos = _positions(positions, market.id)
            yes_price = _clamp_price(mid + edge)
            no_price = _clamp_price((1.0 - mid) + edge)

            yes_qty = _buy_qty(
                budget, yes_price, self.config.max_inventory - yes_pos, remaining_cash
            )
            if yes_qty > 0:
                orders.append(BuyYes.at_price(market.id, yes_price, yes_qty))
                remaining_cash -= yes_qty * yes_price

            no_qty = _buy_qty(
                budget, no_price, self.config.max_inventory - no_pos, remaining_cash
            )
            if no_qty > 0:
                orders.append(BuyNo.at_price(market.id, no_price, no_qty))
                remaining_cash -= no_qty * no_price

        return orders


class CrossingNoiseTrader(BaseAgent):
    """BaseAgent adapter for :class:`CrossingNoiseStrategy`."""

    def __init__(
        self,
        client,
        account_id: int,
        *,
        markets_info: dict[int, Market],
        config: SyntheticStrategyConfig | None = None,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name or "CrossingNoiseTrader", market_ids)
        self.markets_info = markets_info
        self.strategy = CrossingNoiseStrategy(config or SyntheticStrategyConfig())

    async def on_block(self, block: Block) -> list[OrderSpec]:
        markets = self.markets_info
        if self.market_ids is not None:
            markets = {
                market_id: market
                for market_id, market in markets.items()
                if market_id in self.market_ids
            }
        return self.strategy.generate_orders(
            block, markets, self.positions, self.current_balance
        )


class NativeNoiseTrader(BaseAgent):
    """BaseAgent adapter for :class:`NativeNoiseStrategy`."""

    def __init__(
        self,
        client,
        account_id: int,
        *,
        markets_info: dict[int, Market],
        config: SyntheticStrategyConfig | None = None,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name or "NativeNoiseTrader", market_ids)
        self.markets_info = markets_info
        self.strategy = NativeNoiseStrategy(config or SyntheticStrategyConfig())

    async def on_block(self, block: Block) -> list[OrderSpec]:
        markets = self.markets_info
        if self.market_ids is not None:
            markets = {
                market_id: market
                for market_id, market in markets.items()
                if market_id in self.market_ids
            }
        return self.strategy.generate_orders(
            block, markets, self.positions, self.current_balance
        )
