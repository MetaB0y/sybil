"""Deterministic synthetic traders for live mirror/native markets.

The mirror market maker lives in ``sybil-polymarket``. These arena strategies
are the lightweight taker flow around it:

- reference-backed fast traders move Sybil prices toward Polymarket;
- crossing noise creates sparse opposing flow across the full live universe;
- native-only noise remains the explicit non-crossing fallback.
"""

from __future__ import annotations

import math
import random
import time
from dataclasses import dataclass, replace
from hashlib import sha256
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
    # Aggressive sparse noise. Each account submits at most one inventory-aware
    # order per selected market; distinct accounts can cross one another or the
    # MM without manufacturing a same-account complete set.
    crossing_enabled: bool = True
    crossing_edge: float = 0.03
    crossing_markets_per_block: int = 4  # 15 actors × 4 draws ≈ 25% of 206 markets

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
        return self.strategy.generate_orders(block, markets, self.positions, self.current_balance)


class CrossingNoiseStrategy:
    """Aggressive sparse flow that never crosses an account with itself.

    Each actor samples a small market subset and emits at most one order per
    market. Independent seeds create opposing flow across accounts, while
    prices just through the previous mark can also trade with MM quotes.
    """

    def __init__(
        self,
        config: SyntheticStrategyConfig,
        group_members_by_market: dict[int, frozenset[int]] | None = None,
    ):
        self.config = config
        self.group_members_by_market = group_members_by_market or {}

    def _rng_for_block(self, block_height: int) -> random.Random:
        material = f"crossing-noise:{self.config.random_seed}:{block_height}".encode()
        seed = int.from_bytes(sha256(material).digest()[:8], "big")
        return random.Random(seed)

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

        rng = self._rng_for_block(block.height)
        per_block = self.config.crossing_markets_per_block
        if per_block and per_block < len(candidates):
            chosen = rng.sample(candidates, per_block)
        else:
            chosen = candidates

        edge = self.config.crossing_edge
        remaining_cash = cash
        buy_yes_by_group: dict[frozenset[int], int] = {}
        orders: list[OrderSpec] = []
        for market in chosen:
            # Anchor on the previous Sybil price; fresh markets default to 0.5.
            mid = _previous_sybil_price(block, market)
            if mid is None:
                mid = 0.5
            # Small jitter so prices differ across accounts and over time.
            jitter = rng.uniform(
                -self.config.bounded_randomization_range,
                self.config.bounded_randomization_range,
            )
            mid = _clamp_price(mid + jitter)

            yes_pos, no_pos = _positions(positions, market.id)
            group = self.group_members_by_market.get(market.id)
            allow_buy_yes = group is None or buy_yes_by_group.get(group, 0) < len(group) - 1
            order = self._one_order(
                market.id,
                mid,
                yes_pos,
                no_pos,
                remaining_cash,
                edge,
                allow_buy_yes=allow_buy_yes,
                rng=rng,
            )
            if order is None:
                continue
            orders.append(order)
            if isinstance(order, BuyYes) and group is not None:
                buy_yes_by_group[group] = buy_yes_by_group.get(group, 0) + 1
            if isinstance(order, (BuyYes, BuyNo)):
                remaining_cash -= order.quantity * (order.limit_price_nanos / NANOS_PER_DOLLAR)

        return orders

    def _one_order(
        self,
        market_id: int,
        mid: float,
        yes_pos: float,
        no_pos: float,
        cash: float,
        edge: float,
        *,
        allow_buy_yes: bool,
        rng: random.Random,
    ) -> OrderSpec | None:
        budget = self.config.notional_budget
        actions: list[tuple[str, float]] = []
        # The caller suppresses the final YES buy that would complete a whole
        # mutually-exclusive group in one account submission.
        if allow_buy_yes and yes_pos < self.config.max_inventory and cash > 0:
            actions.append(("buy_yes", self.config.max_inventory - yes_pos))
        if no_pos < self.config.max_inventory and cash > 0:
            actions.append(("buy_no", self.config.max_inventory - no_pos))
        if yes_pos > 0:
            actions.append(("sell_yes", yes_pos))
        if no_pos > 0:
            actions.append(("sell_no", no_pos))
        if not actions:
            return None

        action, room = rng.choice(actions)
        if action == "buy_yes":
            price = _clamp_price(mid + edge)
            quantity = _buy_qty(budget, price, room, cash)
            return BuyYes.at_price(market_id, price, quantity) if quantity > 0 else None
        if action == "buy_no":
            price = _clamp_price((1.0 - mid) + edge)
            quantity = _buy_qty(budget, price, room, cash)
            return BuyNo.at_price(market_id, price, quantity) if quantity > 0 else None
        if action == "sell_yes":
            price = _clamp_price(mid - edge)
            quantity = _sell_qty(budget, price, room)
            return SellYes.at_price(market_id, price, quantity) if quantity > 0 else None

        price = _clamp_price((1.0 - mid) - edge)
        quantity = _sell_qty(budget, price, room)
        return SellNo.at_price(market_id, price, quantity) if quantity > 0 else None


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
        group_members_by_market: dict[int, frozenset[int]] | None = None,
    ):
        super().__init__(client, account_id, name or "CrossingNoiseTrader", market_ids)
        self.markets_info = markets_info
        self.strategy = CrossingNoiseStrategy(
            config or SyntheticStrategyConfig(),
            group_members_by_market=group_members_by_market,
        )

    async def on_block(self, block: Block) -> list[OrderSpec]:
        markets = self.markets_info
        if self.market_ids is not None:
            markets = {
                market_id: market
                for market_id, market in markets.items()
                if market_id in self.market_ids
            }
        return self.strategy.generate_orders(block, markets, self.positions, self.current_balance)


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
        return self.strategy.generate_orders(block, markets, self.positions, self.current_balance)
