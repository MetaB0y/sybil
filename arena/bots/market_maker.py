"""Market maker bots.

AnchorMarketMaker: FBA-aware MM using EMA reference price and size-based
  inventory management.  Uses flash liquidity (one-shot orders) to prevent
  cross-block self-trading.
SimpleMarketMaker: Basic MM using per-order balance checks.
FlashMarketMaker: Uses mm_budget_nanos for ~20x capital efficiency.
  The solver picks the welfare-optimal subset that fits the budget.
  MM orders are one-shot (re-quoted every block).
"""

import math

from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes

from .base import BaseAgent

NANOS_PER_DOLLAR = 1_000_000_000


class AnchorMarketMaker(BaseAgent):
    """FBA-aware market maker with inventory-managed quoting.

    Quotes symmetrically around the latest clearing price.  Self-trading is
    impossible because the spread guarantees:
        bid_yes + bid_no = mid - offset + (1-mid) - offset = 1 - 2*offset < $1
        → minting impossible.
        ask_yes + ask_no = mid + offset + (1-mid) + offset = 1 + 2*offset > $1
        → burning impossible.

    Inventory management is through ORDER SIZE, not price skewing.  When
    holding excess YES, reduce BuyYes quantity and increase SellYes quantity.

    Orders are one-shot (flash liquidity) and never carry over.
    """

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 5000.0,
        base_spread: float = 0.05,
        num_levels: int = 3,
        level_spacing: float = 0.01,
        base_size_dollars: float = 300.0,
        max_position: int = 8000,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.base_spread = base_spread
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.base_size_dollars = base_size_dollars
        self.max_position = max_position

    def _inventory_multipliers(
        self, yes_pos: int, no_pos: int
    ) -> tuple[float, float, float, float]:
        """Compute size multipliers based on inventory imbalance.

        Returns (buy_yes_mult, sell_yes_mult, buy_no_mult, sell_no_mult).
        When holding excess YES: reduce BuyYes, boost SellYes (and vice versa).
        Multipliers stay in [0.3, 1.5] so MM always quotes both sides.
        """
        total = yes_pos + no_pos
        if total == 0:
            return (1.0, 1.0, 1.0, 1.0)

        # imbalance in [-1, +1]: positive = excess YES
        raw = (yes_pos - no_pos) / total
        # Gentle curve: tanh(0.8*x) keeps multipliers moderate
        skew = math.tanh(0.8 * raw)  # range roughly [-0.66, +0.66]

        # Scale to [0.3, 1.5] — always quote both sides meaningfully
        buy_yes = max(0.3, 1.0 - 0.7 * skew)
        sell_yes = min(1.5, 1.0 + 0.7 * skew)
        buy_no = max(0.3, 1.0 + 0.7 * skew)
        sell_no = min(1.5, 1.0 - 0.7 * skew)
        return (buy_yes, sell_yes, buy_no, sell_no)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders: list[OrderSpec] = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            mid = yes_nanos / NANOS_PER_DOLLAR
            mid = max(0.05, min(0.95, mid))
            no_mid = 1.0 - mid

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            by_mult, sy_mult, bn_mult, sn_mult = self._inventory_multipliers(
                yes_pos, no_pos
            )

            # Compress spreads near edges so quotes stay in [0.01, 0.99]
            edge_room = min(mid, 1.0 - mid)  # distance to nearest edge
            edge_scale = min(1.0, edge_room / 0.15)  # compress when < 15% from edge

            for level in range(1, self.num_levels + 1):
                offset = (self.base_spread / 2 + (level - 1) * self.level_spacing) * edge_scale

                yes_bid = mid - offset
                yes_ask = mid + offset
                no_bid = no_mid - offset
                no_ask = no_mid + offset

                # --- BuyYes ---
                if yes_bid >= 0.01 and yes_pos < self.max_position:
                    qty = max(1, min(self.max_position, int(self.base_size_dollars * by_mult / yes_bid)))
                    orders.append(BuyYes.at_price(market_id, yes_bid, qty))

                # --- SellYes ---
                if yes_ask <= 0.99 and yes_pos > 0:
                    qty = max(1, min(self.max_position, int(self.base_size_dollars * sy_mult / yes_ask)))
                    qty = min(qty, yes_pos)
                    if qty > 0:
                        orders.append(SellYes.at_price(market_id, yes_ask, qty))

                # --- BuyNo ---
                if no_bid >= 0.01 and no_pos < self.max_position:
                    qty = max(1, min(self.max_position, int(self.base_size_dollars * bn_mult / no_bid)))
                    orders.append(BuyNo.at_price(market_id, no_bid, qty))

                # --- SellNo ---
                if no_ask <= 0.99 and no_pos > 0:
                    qty = max(1, min(self.max_position, int(self.base_size_dollars * sn_mult / no_ask)))
                    qty = min(qty, no_pos)
                    if qty > 0:
                        orders.append(SellNo.at_price(market_id, no_ask, qty))

        return orders


class SimpleMarketMaker(BaseAgent):
    """Quotes both sides of each market with a configurable spread.

    Places buy orders on both YES and NO sides. BuyYes + BuyNo can match
    via minting (total cost = $1). The MM profits from the bid-ask spread.
    Uses per-order balance checks (no flash liquidity).
    """

    def __init__(
        self,
        client,
        account_id: int,
        spread_bps: int = 200,
        quote_size: int = 5,
        max_position: int = 50,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.spread_bps = spread_bps
        self.quote_size = quote_size
        self.max_position = max_position

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            yes_price = yes_nanos / NANOS_PER_DOLLAR
            no_price = no_nanos / NANOS_PER_DOLLAR
            half_spread = (self.spread_bps / 10000) / 2

            yes_bid = max(0.01, yes_price - half_spread)
            no_bid = max(0.01, no_price - half_spread)

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if yes_pos < self.max_position:
                orders.append(BuyYes.at_price(market_id, yes_bid, self.quote_size))
            if no_pos < self.max_position:
                orders.append(BuyNo.at_price(market_id, no_bid, self.quote_size))

            if yes_pos > 0:
                ask_price = min(0.99, yes_price + half_spread)
                sell_qty = min(yes_pos, self.quote_size)
                orders.append(SellYes.at_price(market_id, ask_price, sell_qty))
            if no_pos > 0:
                ask_price = min(0.99, no_price + half_spread)
                sell_qty = min(no_pos, self.quote_size)
                orders.append(SellNo.at_price(market_id, ask_price, sell_qty))

        return orders


class FlashMarketMaker(BaseAgent):
    """Market maker using flash liquidity (portfolio-level budget constraint).

    Submits buy orders on both YES and NO sides at multiple price levels.
    Total notional can far exceed the MM's balance because the solver only
    activates the welfare-optimal subset that fits the budget.
    """

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 1000.0,
        num_levels: int = 3,
        level_spacing_cents: int = 3,
        dollars_per_level: float = 500.0,
        skew_factor: float = 0.1,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.num_levels = num_levels
        self.level_spacing_cents = level_spacing_cents
        self.dollars_per_level = dollars_per_level
        self.skew_factor = skew_factor

    def _compute_skew(self, market_id: int) -> float:
        """Inventory skew: excess YES → shift mid DOWN to rebalance."""
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        return (no_pos - yes_pos) * self.skew_factor * 0.01

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []
        spacing = self.level_spacing_cents / 100  # e.g. 3 cents = 0.03

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            yes_mid = yes_nanos / NANOS_PER_DOLLAR
            skew = self._compute_skew(market_id)
            yes_mid = max(0.05, min(0.95, yes_mid + skew))
            no_mid = max(0.05, min(0.95, 1.0 - yes_mid))

            for level in range(1, self.num_levels + 1):
                offset = level * spacing
                yes_bid = max(0.01, yes_mid - offset)
                no_bid = max(0.01, no_mid - offset)
                yes_qty = max(1, int(self.dollars_per_level / yes_bid))
                no_qty = max(1, int(self.dollars_per_level / no_bid))
                orders.append(BuyYes.at_price(market_id, yes_bid, yes_qty))
                orders.append(BuyNo.at_price(market_id, no_bid, no_qty))

        return orders


class BalancedMarketMaker(BaseAgent):
    """Two-sided market maker with inventory-aware quoting.

    Quotes buy and sell on both YES and NO at multiple price levels.
    Total notional capped at risk_fraction of portfolio value.
    Uses regular balance checks (no flash liquidity).
    """

    def __init__(
        self,
        client,
        account_id: int,
        num_levels: int = 3,
        level_spacing_cents: int = 3,
        risk_fraction: float = 0.30,
        skew_factor: float = 0.1,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.num_levels = num_levels
        self.level_spacing_cents = level_spacing_cents
        self.risk_fraction = risk_fraction
        self.skew_factor = skew_factor
        # mm_budget_nanos stays None — no flash liquidity

    def _compute_skew(self, market_id: int) -> float:
        """Inventory skew: excess YES → shift mid DOWN to sell YES faster.

        Uses tanh to bound the skew to ±max_skew regardless of position size.
        """
        import math
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        imbalance = yes_pos - no_pos
        max_skew = 0.15
        # Scale so ~500 imbalance produces ~half of max_skew
        normalized = imbalance * self.skew_factor * 0.01 / max_skew
        return max_skew * math.tanh(normalized)

    def _portfolio_value(self, market_id: int, mid: float) -> float:
        """cash + yes_pos * mid + no_pos * (1 - mid)"""
        cash = self.current_balance
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        return cash + yes_pos * mid + no_pos * (1 - mid)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []
        spacing = self.level_spacing_cents / 100
        remaining_cash = self.current_balance

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            yes_mid = yes_nanos / NANOS_PER_DOLLAR
            skew = self._compute_skew(market_id)
            adjusted_mid = max(0.05, min(0.95, yes_mid - skew))
            no_mid = 1.0 - adjusted_mid

            portfolio_val = self._portfolio_value(market_id, adjusted_mid)
            risk_budget = self.risk_fraction * portfolio_val
            num_slots = self.num_levels * 4  # buy/sell × yes/no
            dollars_per_slot = risk_budget / num_slots if num_slots > 0 else 0

            remaining_yes = self.get_position(market_id, "YES")
            remaining_no = self.get_position(market_id, "NO")

            for level in range(1, self.num_levels + 1):
                offset = level * spacing

                # YES Bid (BuyYes)
                yes_bid = adjusted_mid - offset
                if yes_bid >= 0.01 and remaining_cash > 0:
                    qty = min(
                        int(dollars_per_slot / yes_bid),
                        int(remaining_cash / yes_bid),
                    )
                    if qty > 0:
                        orders.append(BuyYes.at_price(market_id, yes_bid, qty))
                        remaining_cash -= qty * yes_bid

                # YES Ask (SellYes)
                yes_ask = adjusted_mid + offset
                if yes_ask <= 0.99 and remaining_yes > 0:
                    qty = min(
                        int(dollars_per_slot / yes_ask),
                        remaining_yes,
                    )
                    if qty > 0:
                        orders.append(SellYes.at_price(market_id, yes_ask, qty))
                        remaining_yes -= qty

                # NO Bid (BuyNo)
                no_bid = no_mid - offset
                if no_bid >= 0.01 and remaining_cash > 0:
                    qty = min(
                        int(dollars_per_slot / no_bid),
                        int(remaining_cash / no_bid),
                    )
                    if qty > 0:
                        orders.append(BuyNo.at_price(market_id, no_bid, qty))
                        remaining_cash -= qty * no_bid

                # NO Ask (SellNo)
                no_ask = no_mid + offset
                if no_ask <= 0.99 and remaining_no > 0:
                    qty = min(
                        int(dollars_per_slot / no_ask),
                        remaining_no,
                    )
                    if qty > 0:
                        orders.append(SellNo.at_price(market_id, no_ask, qty))
                        remaining_no -= qty

        return orders


class TightFlashMM(FlashMarketMaker):
    """Tight-spread flash MM. Profits from volume, vulnerable to adverse selection."""

    def __init__(self, client, account_id: int, name: str | None = None,
                 market_ids: list[int] | None = None, budget_dollars: float = 1000.0):
        super().__init__(
            client=client, account_id=account_id, budget_dollars=budget_dollars,
            num_levels=4, level_spacing_cents=1, dollars_per_level=500.0,
            skew_factor=0.15, name=name, market_ids=market_ids,
        )


class WideFlashMM(FlashMarketMaker):
    """Wide-spread flash MM. Less volume but survives adverse selection better."""

    def __init__(self, client, account_id: int, name: str | None = None,
                 market_ids: list[int] | None = None, budget_dollars: float = 1000.0):
        super().__init__(
            client=client, account_id=account_id, budget_dollars=budget_dollars,
            num_levels=3, level_spacing_cents=5, dollars_per_level=300.0,
            skew_factor=0.05, name=name, market_ids=market_ids,
        )
