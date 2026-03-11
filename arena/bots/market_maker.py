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
    """FBA-aware market maker with price-skewing inventory management.

    Places one BuyYes and one BuyNo per block at ±spread from clearing price.
    Self-trading is impossible because:
        bid_yes + bid_no = mid - spread + (1-mid) - spread = 1 - 2*spread < $1

    Inventory management via price skewing (NBA-style): when long YES, shift
    mid DOWN so BuyYes becomes less attractive and BuyNo more attractive.

    Spread widens automatically after large price moves (volatility detection).

    Orders are one-shot (flash liquidity) and never carry over.
    """

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 5000.0,
        half_spread: float = 0.015,
        base_size_dollars: float = 300.0,
        max_position: int = 8000,
        skew_factor: float = 0.03,
        vol_lookback: int = 5,
        vol_widen_mult: float = 3.0,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.half_spread = half_spread
        self.base_size_dollars = base_size_dollars
        self.max_position = max_position
        self.skew_factor = skew_factor
        self.vol_lookback = vol_lookback
        self.vol_widen_mult = vol_widen_mult
        # Price history per market for volatility detection
        self._price_history: dict[int, list[float]] = {}

    def _compute_skew(self, yes_pos: int, no_pos: int) -> float:
        """Price skew based on inventory imbalance.

        When long YES (net > 0), returns negative skew → shifts mid DOWN
        → BuyYes cheaper (less likely to fill), BuyNo more expensive (more fills)
        → market pushes inventory back toward balance.
        """
        net = yes_pos - no_pos
        if self.max_position == 0:
            return 0.0
        raw = net / self.max_position  # [-1, +1]
        return -math.tanh(raw) * self.skew_factor

    def _vol_multiplier(self, market_id: int, mid: float) -> float:
        """Widen spread after large price moves."""
        history = self._price_history.setdefault(market_id, [])
        history.append(mid)
        # Keep only lookback window
        if len(history) > self.vol_lookback + 1:
            del history[:-self.vol_lookback - 1]
        if len(history) < 3:
            return 1.0
        recent = history[-self.vol_lookback:]
        vol = max(recent) - min(recent)
        if vol > 0.05:
            return min(self.vol_widen_mult, 1.0 + vol * 5)
        return 1.0

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders: list[OrderSpec] = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            mid = yes_nanos / NANOS_PER_DOLLAR
            mid = max(0.02, min(0.98, mid))

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            # Price skew: shift mid based on inventory
            skew = self._compute_skew(yes_pos, no_pos)
            skewed_mid = max(0.02, min(0.98, mid + skew))

            # Volatility-aware spread
            vol_mult = self._vol_multiplier(market_id, mid)
            spread = self.half_spread * vol_mult

            # Compress spread near edges so quotes stay in [0.01, 0.99]
            edge_room = min(skewed_mid, 1.0 - skewed_mid)
            spread = min(spread, edge_room - 0.01)

            yes_bid = skewed_mid - spread
            no_bid = (1.0 - skewed_mid) - spread

            # BuyYes
            if yes_bid >= 0.01 and yes_pos < self.max_position:
                qty = max(1, int(self.base_size_dollars / yes_bid))
                orders.append(BuyYes.at_price(market_id, yes_bid, qty))

            # BuyNo
            if no_bid >= 0.01 and no_pos < self.max_position:
                qty = max(1, int(self.base_size_dollars / no_bid))
                orders.append(BuyNo.at_price(market_id, no_bid, qty))

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
    """FBA-compatible two-sided market maker with no self-trading.

    Core rule: per outcome, only be on ONE side (buy or sell) per block.
    Uses flash liquidity (mm_budget_nanos) for one-shot orders — no TTL
    carryover prevents cross-block self-trading.

    Three modes based on inventory:
      BALANCED: BuyYes + BuyNo (minting — standard MM liquidity)
      LONG_YES: SellYes + BuyNo (offload YES, absorb NO)
      LONG_NO:  BuyYes + SellNo (absorb YES, offload NO)
    """

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 50_000.0,
        half_spread: float = 0.03,
        num_levels: int = 3,
        level_spacing_cents: int = 3,
        base_size_dollars: float = 250.0,
        max_position: int = 15_000,
        sell_threshold_pct: float = 0.10,
        skew_factor: float = 0.1,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.half_spread = half_spread
        self.num_levels = num_levels
        self.level_spacing = level_spacing_cents / 100
        self.base_size_dollars = base_size_dollars
        self.max_position = max_position
        self.sell_threshold = int(max_position * sell_threshold_pct)
        self.skew_factor = skew_factor

    def _compute_skew(self, net: int, mid: float) -> float:
        """Price skew based on inventory imbalance.

        Capped at 20% of distance-from-edge to avoid distorting low-prob markets.
        E.g. mid=0.20 → max skew ±0.04, mid=0.50 → max skew ±0.10.
        """
        max_skew = min(mid, 1.0 - mid) * 0.20
        if max_skew < 0.005 or self.max_position == 0:
            return 0.0
        normalized = net * self.skew_factor * 0.01 / max_skew
        return max_skew * math.tanh(normalized)

    def _buy_orders(
        self, market_id: int, mid: float, is_yes: bool, remaining_cash: float,
    ) -> tuple[list[OrderSpec], float]:
        """Generate buy orders for one side. Returns (orders, cash_remaining)."""
        orders = []
        cls = BuyYes if is_yes else BuyNo
        for level in range(1, self.num_levels + 1):
            bid = mid - level * self.level_spacing
            if bid < 0.01 or remaining_cash <= 0:
                continue
            qty = min(
                int(self.base_size_dollars / bid),
                int(remaining_cash / bid),
                self.max_position,
            )
            if qty > 0:
                orders.append(cls.at_price(market_id, bid, qty))
                remaining_cash -= qty * bid
        return orders, remaining_cash

    def _sell_orders(
        self, market_id: int, mid: float, is_yes: bool, held: int,
    ) -> list[OrderSpec]:
        """Generate sell orders for one side. Tapered: most at best ask."""
        orders = []
        cls = SellYes if is_yes else SellNo
        remaining = held
        # Sell taper weights: 50%, 30%, 20% at levels 1, 2, 3
        weights = [0.50, 0.30, 0.20]
        for level in range(1, self.num_levels + 1):
            ask = mid + level * self.level_spacing
            if ask > 0.99 or remaining <= 0:
                continue
            w = weights[level - 1] if level <= len(weights) else weights[-1]
            qty = max(1, min(int(held * w), remaining))
            if qty > 0:
                orders.append(cls.at_price(market_id, ask, qty))
                remaining -= qty
        return orders

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders: list[OrderSpec] = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            yes_mid = yes_nanos / NANOS_PER_DOLLAR
            yes_mid = max(0.05, min(0.95, yes_mid))

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            net = yes_pos - no_pos  # positive = long YES

            # Skew mid based on inventory
            skew = self._compute_skew(net, yes_mid)
            adjusted_yes_mid = max(0.05, min(0.95, yes_mid - skew))
            adjusted_no_mid = 1.0 - adjusted_yes_mid

            # Compress spread near edges
            edge_room = min(adjusted_yes_mid, adjusted_no_mid)
            spread = min(self.half_spread, edge_room - 0.01)
            if spread < 0.005:
                continue

            remaining_cash = self.current_balance

            # Buy orders at mid - spread - (level-1)*spacing
            # Level 1 (tightest): mid - spread
            # Level 2: mid - spread - spacing, etc.
            # Minting sum at level 1: (yes_mid - spread) + (no_mid - spread)
            #                       = 1.0 - 2*spread < $1 (MM profit from spread)
            # Sell orders at mid + spread + (level-1)*spacing
            yes_buy_center = adjusted_yes_mid - spread + self.level_spacing
            no_buy_center = adjusted_no_mid - spread + self.level_spacing
            yes_sell_center = adjusted_yes_mid + spread - self.level_spacing
            no_sell_center = adjusted_no_mid + spread - self.level_spacing

            if net > self.sell_threshold:
                # LONG YES: sell YES + buy NO
                orders.extend(self._sell_orders(
                    market_id, yes_sell_center, True, yes_pos))
                buy_orders, _ = self._buy_orders(
                    market_id, no_buy_center, False, remaining_cash)
                orders.extend(buy_orders)

            elif net < -self.sell_threshold:
                # LONG NO: buy YES + sell NO
                buy_orders, _ = self._buy_orders(
                    market_id, yes_buy_center, True, remaining_cash)
                orders.extend(buy_orders)
                orders.extend(self._sell_orders(
                    market_id, no_sell_center, False, no_pos))

            else:
                # BALANCED: buy YES + buy NO (minting mode)
                yes_orders, remaining_cash = self._buy_orders(
                    market_id, yes_buy_center, True, remaining_cash)
                no_orders, _ = self._buy_orders(
                    market_id, no_buy_center, False, remaining_cash)
                orders.extend(yes_orders)
                orders.extend(no_orders)

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
