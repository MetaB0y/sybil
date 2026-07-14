"""Data types for Sybil client."""

from dataclasses import dataclass, field
from decimal import Decimal, ROUND_FLOOR
from typing import Literal

NANOS_PER_DOLLAR = 1_000_000_000
SHARE_SCALE = 1_000
TimeInForce = Literal["GTC", "IOC", "GTD"]


def shares_to_quantity_units(shares: int | float | Decimal) -> int:
    """Convert user-facing shares to protocol share-units."""
    units = (Decimal(str(shares)) * SHARE_SCALE).to_integral_value(rounding=ROUND_FLOOR)
    return max(0, int(units))


def quantity_units_to_shares(quantity_units: int) -> float:
    """Convert protocol share-units to user-facing shares."""
    return quantity_units / SHARE_SCALE


@dataclass
class Position:
    """A position in a market outcome."""

    market_id: int
    outcome: Literal["YES", "NO"]
    quantity: float


@dataclass
class Account:
    """Account with balance and positions."""

    id: int
    balance_nanos: int
    positions: list[Position] = field(default_factory=list)

    @property
    def balance_dollars(self) -> float:
        return self.balance_nanos / NANOS_PER_DOLLAR

    def position(self, market_id: int, outcome: str) -> float:
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
    reference_price_nanos: int | None = None
    polymarket_condition_id: str | None = None
    description: str = ""
    category: str = ""
    tags: list[str] = field(default_factory=list)
    resolution_criteria: str = ""
    expiry_timestamp_ms: int = 0
    created_at_ms: int = 0
    volume_nanos: int = 0
    actor_min_yes_nanos: int | None = None
    actor_max_yes_nanos: int | None = None
    actor_seed_yes_nanos: int | None = None

    @property
    def yes_price(self) -> float:
        return self.yes_price_nanos / NANOS_PER_DOLLAR

    @property
    def no_price(self) -> float:
        return self.no_price_nanos / NANOS_PER_DOLLAR

    @property
    def reference_price(self) -> float | None:
        if self.reference_price_nanos is None:
            return None
        return self.reference_price_nanos / NANOS_PER_DOLLAR

    @property
    def volume_dollars(self) -> float:
        return self.volume_nanos / NANOS_PER_DOLLAR


@dataclass
class Fill:
    """A fill from the matching engine."""

    order_id: int
    fill_qty: float
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


@dataclass
class PricePoint:
    """A single price observation at a given block."""

    height: int
    timestamp_ms: int
    yes_price_nanos: int
    no_price_nanos: int
    volume_nanos: int

    @property
    def yes_price(self) -> float:
        return self.yes_price_nanos / NANOS_PER_DOLLAR

    @property
    def no_price(self) -> float:
        return self.no_price_nanos / NANOS_PER_DOLLAR


@dataclass
class PositionDelta:
    """A position change from a fill."""

    market_id: int
    outcome: str
    delta: float


@dataclass
class AccountFill:
    """Record of a fill attributed to an account."""

    order_id: int
    fill_qty: float
    fill_price_nanos: int
    block_height: int
    timestamp_ms: int
    position_deltas: list[PositionDelta] = field(default_factory=list)
    cursor: str = ""

    @property
    def fill_price(self) -> float:
        return self.fill_price_nanos / NANOS_PER_DOLLAR


@dataclass
class PendingOrder:
    """Pending order currently reserving account balance or positions."""

    order_id: int
    account_id: int
    market_id: int
    side: str
    limit_price_nanos: int
    remaining_quantity: float
    created_at_block: int
    expires_at_block: int | None
    original_quantity: float

    @property
    def limit_price(self) -> float:
        return self.limit_price_nanos / NANOS_PER_DOLLAR


@dataclass
class PositionValue:
    """A position valued at current market prices."""

    market_id: int
    outcome: str
    quantity: float
    current_price_nanos: int
    value_nanos: int

    @property
    def value_dollars(self) -> float:
        return self.value_nanos / NANOS_PER_DOLLAR


@dataclass
class Portfolio:
    """Portfolio summary with valued positions and PnL."""

    account_id: int
    balance_nanos: int
    total_deposited_nanos: int
    positions: list[PositionValue]
    total_position_value_nanos: int
    portfolio_value_nanos: int
    pnl_nanos: int

    @property
    def balance_dollars(self) -> float:
        return self.balance_nanos / NANOS_PER_DOLLAR

    @property
    def pnl_dollars(self) -> float:
        return self.pnl_nanos / NANOS_PER_DOLLAR

    @property
    def portfolio_value_dollars(self) -> float:
        return self.portfolio_value_nanos / NANOS_PER_DOLLAR


# Order specifications for submission
@dataclass
class OrderSpec:
    """Base class for order specifications."""

    pass


@dataclass
class BuyYes(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: float

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int | float) -> "BuyYes":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class BuyNo(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: float

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int | float) -> "BuyNo":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class SellYes(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: float

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int | float) -> "SellYes":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)


@dataclass
class SellNo(OrderSpec):
    market_id: int
    limit_price_nanos: int
    quantity: float

    @classmethod
    def at_price(cls, market_id: int, price: float, quantity: int | float) -> "SellNo":
        return cls(market_id, int(price * NANOS_PER_DOLLAR), quantity)
