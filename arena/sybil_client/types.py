"""Data types for Sybil client."""

from dataclasses import dataclass, field
from typing import Literal

NANOS_PER_DOLLAR = 1_000_000_000


@dataclass
class Position:
    """A position in a market outcome."""

    market_id: int
    outcome: Literal["YES", "NO"]
    quantity: int


@dataclass
class Account:
    """Account with balance and positions."""

    id: int
    balance_nanos: int
    positions: list[Position] = field(default_factory=list)

    @property
    def balance_dollars(self) -> float:
        return self.balance_nanos / NANOS_PER_DOLLAR

    def position(self, market_id: int, outcome: str) -> int:
        """Get position quantity for a market outcome."""
        for pos in self.positions:
            if pos.market_id == market_id and pos.outcome == outcome:
                return pos.quantity
        return 0


@dataclass
class Market:
    """A binary prediction market."""

    id: int
    name: str
    yes_price_nanos: int
    no_price_nanos: int
    status: str

    @property
    def yes_price(self) -> float:
        return self.yes_price_nanos / NANOS_PER_DOLLAR

    @property
    def no_price(self) -> float:
        return self.no_price_nanos / NANOS_PER_DOLLAR


@dataclass
class Fill:
    """A fill from the matching engine."""

    order_id: int
    fill_qty: int
    fill_price_nanos: int

    @property
    def fill_price(self) -> float:
        return self.fill_price_nanos / NANOS_PER_DOLLAR


@dataclass
class Block:
    """A block from the sequencer."""

    height: int
    parent_hash: str
    state_root: str
    fills: list[Fill]
    clearing_prices: dict[int, tuple[int, int]]  # market_id -> (yes_nanos, no_nanos)
    total_welfare: int
    total_volume: int
    orders_filled: int

    def price_for(self, market_id: int) -> tuple[float, float] | None:
        """Get (yes_price, no_price) for a market."""
        if market_id in self.clearing_prices:
            yes_nanos, no_nanos = self.clearing_prices[market_id]
            return yes_nanos / NANOS_PER_DOLLAR, no_nanos / NANOS_PER_DOLLAR
        return None


# Order specifications for submission
@dataclass
class OrderSpec:
    """Base class for order specifications."""

    pass


@dataclass
class BuyYes(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: int

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int) -> "BuyYes":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class BuyNo(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: int

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int) -> "BuyNo":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class SellYes(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: int

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int) -> "SellYes":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class SellNo(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: int

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int) -> "SellNo":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)
