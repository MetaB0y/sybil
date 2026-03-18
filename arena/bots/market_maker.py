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
        max_blocks: int | None = None,
    ):
        super().__init__(client, account_id, name, market_ids, max_blocks=max_blocks)
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
        max_blocks: int | None = None,
    ):
        super().__init__(client, account_id, name, market_ids, max_blocks=max_blocks)
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
        max_blocks: int | None = None,
    ):
        super().__init__(client, account_id, name, market_ids, max_blocks=max_blocks)
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
    """FBA-native mean-reversion market maker.

    Core design: profits from mean reversion via a slow anchor price and
    deviation-based asymmetric quoting. Only buys the cheap side (relative
    to anchor) and only sells when price is favorable. Skips buying during
    momentum or large deviations to avoid adverse selection.

    Tight inner spread (1c) maximizes fill rate against noise. Vol-adaptive
    widening and patient inventory management protect against informed traders.
    """

    # Taper weights per level (inner → outer). Sum ≈ 1.0.
    _BUY_TAPER = [0.50, 0.30, 0.20]
    _SELL_TAPER = [0.50, 0.30, 0.20]

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 50_000.0,
        half_spread: float = 0.01,
        num_levels: int = 3,
        level_spacing: float = 0.015,
        max_per_side_dollars: float = 100.0,
        max_position: int = 5_000,
        skew_factor: float = 0.1,
        vol_lookback: int = 4,
        vol_widen_max: float = 6.0,
        inventory_decay_start: int = 30,
        anchor_alpha: float = 0.03,
        fast_alpha: float = 0.15,
        deviation_threshold: float = 0.05,
        name: str | None = None,
        market_ids: list[int] | None = None,
        max_blocks: int | None = None,
    ):
        super().__init__(client, account_id, name, market_ids, max_blocks=max_blocks)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.half_spread = half_spread
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_per_side = max_per_side_dollars
        self.max_position = max_position
        self.skew_factor = skew_factor
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.inventory_decay_start = inventory_decay_start
        self.anchor_alpha = anchor_alpha
        self.fast_alpha = fast_alpha
        self.deviation_threshold = deviation_threshold
        self._price_history: dict[int, list[float]] = {}
        self._anchor: dict[int, float] = {}
        self._fast_ema: dict[int, float] = {}

    def _update_prices(self, market_id: int, price: float) -> tuple[float, float]:
        """Update and return (anchor, fast_ema)."""
        if market_id not in self._anchor:
            self._anchor[market_id] = price
            self._fast_ema[market_id] = price
            return price, price
        self._anchor[market_id] += self.anchor_alpha * (price - self._anchor[market_id])
        self._fast_ema[market_id] += self.fast_alpha * (price - self._fast_ema[market_id])
        return self._anchor[market_id], self._fast_ema[market_id]

    def _vol_multiplier(self, market_id: int, mid: float) -> float:
        """Widen spread after large price moves.

        Two signals: windowed range + single-block jump (amplified 2x).
        Returns multiplier for spread widening. Also used inversely for
        size scaling — high vol → wide spread AND small size.
        """
        history = self._price_history.setdefault(market_id, [])
        history.append(mid)
        if len(history) > self.vol_lookback + 1:
            del history[:-self.vol_lookback - 1]
        if len(history) < 2:
            return 1.0
        recent = history[-self.vol_lookback:]
        windowed_vol = max(recent) - min(recent)
        single_jump = abs(history[-1] - history[-2]) * 2.0
        effective_vol = max(windowed_vol, single_jump)
        if effective_vol > 0.01:
            return min(self.vol_widen_max, 1.0 + effective_vol * 15)
        return 1.0

    def _should_skip(self, market_id: int, raw_price: float, anchor: float) -> bool:
        """Skip buying entirely when conditions are unfavorable."""
        history = self._price_history.get(market_id, [])

        # Skip after large single-block move (existing guard)
        if len(history) >= 2 and abs(history[-1] - history[-2]) > 0.05:
            return True

        # Skip when price is far from anchor (>8c) — the move already happened,
        # adverse selection risk is highest
        if abs(raw_price - anchor) > 0.08:
            return True

        # Skip during sustained directional momentum (3+ blocks same direction)
        if len(history) >= 4:
            deltas = [history[i] - history[i - 1] for i in range(-3, 0)]
            if all(d > 0.005 for d in deltas) or all(d < -0.005 for d in deltas):
                return True

        return False

    def _deviation_scales(self, raw_price: float, anchor: float) -> tuple[float, float]:
        """Scale buy sizes based on price deviation from anchor.

        Returns (yes_buy_scale, no_buy_scale) in [0, 1].
        When price > anchor (YES expensive): suppress BuyYes, boost BuyNo
        When price < anchor (YES cheap): boost BuyYes, suppress BuyNo
        """
        deviation = raw_price - anchor
        # Normalize: deviation_threshold deviation = full suppression of one side
        intensity = min(1.0, abs(deviation) / self.deviation_threshold)

        if deviation > 0:  # price above anchor — YES is expensive
            yes_scale = max(0.0, 1.0 - intensity * 2.0)  # suppress buying YES
            no_scale = 1.0  # happy to buy NO (= sell YES exposure)
        else:  # price below anchor — YES is cheap
            yes_scale = 1.0  # happy to buy YES
            no_scale = max(0.0, 1.0 - intensity * 2.0)  # suppress buying NO

        return yes_scale, no_scale

    def _should_sell(self, raw_price: float, anchor: float, is_yes: bool) -> bool:
        """Only sell when price is above anchor (favorable for the seller).

        If holding YES, only sell when YES price > anchor (selling expensive YES).
        If holding NO, only sell when NO price > (1-anchor), i.e. YES price < anchor.
        """
        if is_yes:
            return raw_price >= anchor  # YES is at/above fair → good time to sell YES
        else:
            return raw_price <= anchor  # YES is at/below fair → NO is at/above fair

    def _matched_pair_penalty(self, yes_pos: int, no_pos: int) -> float:
        """Extra spread multiplier when too much capital is locked in matched pairs."""
        matched = min(yes_pos, no_pos)
        # Each matched pair locks ~$1. Penalty kicks in when >10% of budget is locked.
        budget = self.mm_budget_nanos / NANOS_PER_DOLLAR
        locked_frac = matched / budget if budget > 0 else 0
        if locked_frac > 0.10:
            return 1.0 + min(1.0, (locked_frac - 0.10) * 5)  # up to 2x at 30%
        return 1.0

    def _compute_skew(self, net: int, mid: float) -> float:
        """Price skew based on inventory imbalance."""
        max_skew = min(mid, 1.0 - mid) * 0.20
        if max_skew < 0.005 or self.max_position == 0:
            return 0.0
        normalized = net * self.skew_factor * 0.01 / max_skew
        return max_skew * math.tanh(normalized)

    def _buy_orders(
        self, market_id: int, mid: float, spread: float,
        is_yes: bool, budget: float, scale: float = 1.0,
    ) -> list[OrderSpec]:
        """Generate tapered buy orders for one side, respecting per-side budget."""
        orders = []
        cls = BuyYes if is_yes else BuyNo
        spent = 0.0
        for level in range(self.num_levels):
            bid = mid - spread - level * self.level_spacing
            if bid < 0.01:
                break
            w = self._BUY_TAPER[level] if level < len(self._BUY_TAPER) else self._BUY_TAPER[-1]
            level_dollars = self.max_per_side * w * scale
            room = budget - spent
            if room <= 0:
                break
            level_dollars = min(level_dollars, room)
            qty = int(level_dollars / bid)
            if qty > 0:
                orders.append(cls.at_price(market_id, bid, qty))
                spent += qty * bid
        return orders

    def _sell_orders(
        self, market_id: int, mid: float, spread: float,
        is_yes: bool, held: int,
    ) -> list[OrderSpec]:
        """Generate tapered sell orders for one side."""
        orders = []
        cls = SellYes if is_yes else SellNo
        remaining = held
        for level in range(self.num_levels):
            ask = mid + spread + level * self.level_spacing
            if ask > 0.99 or remaining <= 0:
                break
            w = self._SELL_TAPER[level] if level < len(self._SELL_TAPER) else self._SELL_TAPER[-1]
            qty = max(1, min(int(held * w), remaining))
            orders.append(cls.at_price(market_id, ask, qty))
            remaining -= qty
        return orders

    def _inventory_fractions(
        self, net: int, yes_pos: int, no_pos: int,
    ) -> tuple[float, float, float, float]:
        """Return (yes_buy_scale, no_buy_scale, net_sell_frac, matched_sell_frac).

        Two sell mechanisms:
        1. net_sell_frac: sell the net-long side to reduce directional risk
        2. matched_sell_frac: sell BOTH sides to unlock capital trapped in matched pairs
        """
        matched = min(yes_pos, no_pos)
        total = yes_pos + no_pos

        # --- Matched-pair unwinding ---
        # When matched pairs exceed threshold, sell both sides to free capital.
        # Ramps from 0% at 100 matched to 60% at max_position matched.
        matched_threshold = 50
        if matched > matched_threshold:
            matched_intensity = min(1.0, matched / self.max_position)
            matched_sell = min(0.60, matched_intensity * 1.5)
        else:
            matched_sell = 0.0

        # --- Total inventory pressure on buying ---
        # Reduce buying on BOTH sides when total inventory is high,
        # regardless of net balance.
        total_threshold = 100
        if total > total_threshold:
            total_intensity = min(1.0, total / (self.max_position * 2))
            buy_dampen = max(0.1, 1.0 - total_intensity * 2.0)
        else:
            buy_dampen = 1.0

        # --- Net imbalance ---
        if abs(net) < self.inventory_decay_start:
            return buy_dampen, buy_dampen, 0.0, matched_sell

        intensity = min(1.0, abs(net) / self.max_position)
        net_sell = min(0.80, intensity * 3.0)

        if net > 0:
            yes_scale = max(0.0, buy_dampen * (1.0 - intensity * 3.0))
            no_scale = buy_dampen
        else:
            yes_scale = buy_dampen
            no_scale = max(0.0, buy_dampen * (1.0 - intensity * 3.0))

        return yes_scale, no_scale, net_sell, matched_sell

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders: list[OrderSpec] = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            raw_yes = yes_nanos / NANOS_PER_DOLLAR
            raw_yes = max(0.05, min(0.95, raw_yes))
            anchor, fast_ema = self._update_prices(market_id, raw_yes)

            # Use anchor as the MM's reference mid
            yes_mid = anchor

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            net = yes_pos - no_pos  # positive = long YES

            # Skew mid based on inventory
            skew = self._compute_skew(net, yes_mid)
            adjusted_yes_mid = max(0.05, min(0.95, yes_mid - skew))
            adjusted_no_mid = 1.0 - adjusted_yes_mid

            # Adaptive spread: base × volatility × matched-pair penalty
            # Use raw price for vol detection so it sees actual jumps
            vol_mult = self._vol_multiplier(market_id, raw_yes)
            pair_mult = self._matched_pair_penalty(yes_pos, no_pos)
            spread = self.half_spread * vol_mult * pair_mult

            # Compress near edges
            edge_room = min(adjusted_yes_mid, adjusted_no_mid)
            spread = min(spread, edge_room - 0.01)
            if spread < 0.005:
                continue

            # Continuous inventory management
            yes_buy_scale, no_buy_scale, net_sell, matched_sell = \
                self._inventory_fractions(net, yes_pos, no_pos)

            # Deviation-based asymmetric quoting (mean reversion)
            skip = self._should_skip(market_id, raw_yes, anchor)
            dev_yes, dev_no = self._deviation_scales(raw_yes, anchor)
            yes_buy_scale *= dev_yes
            no_buy_scale *= dev_no

            # Vol-inverse size scaling: high vol → deploy less capital
            effective_per_side = self.max_per_side / vol_mult
            budget = min(self.current_balance, effective_per_side)

            # Buy orders (both sides, scaled by inventory + deviation)
            if not skip and yes_buy_scale > 0:
                orders.extend(self._buy_orders(
                    market_id, adjusted_yes_mid, spread,
                    True, budget, yes_buy_scale))
            if not skip and no_buy_scale > 0:
                orders.extend(self._buy_orders(
                    market_id, adjusted_no_mid, spread,
                    False, budget, no_buy_scale))

            # Sell orders: net imbalance (sell the long side) — patience gated
            if net_sell > 0 and net > 0 and yes_pos > 0:
                if self._should_sell(raw_yes, anchor, is_yes=True):
                    sell_qty = max(1, int(yes_pos * net_sell))
                    orders.extend(self._sell_orders(
                        market_id, adjusted_yes_mid, spread,
                        True, sell_qty))
            elif net_sell > 0 and net < 0 and no_pos > 0:
                if self._should_sell(raw_yes, anchor, is_yes=False):
                    sell_qty = max(1, int(no_pos * net_sell))
                    orders.extend(self._sell_orders(
                        market_id, adjusted_no_mid, spread,
                        False, sell_qty))

            # Sell orders: matched-pair unwinding — also patience gated
            if matched_sell > 0:
                matched = min(yes_pos, no_pos)
                unwind_qty = max(1, int(matched * matched_sell))
                if yes_pos >= unwind_qty and self._should_sell(raw_yes, anchor, is_yes=True):
                    orders.extend(self._sell_orders(
                        market_id, adjusted_yes_mid, spread,
                        True, unwind_qty))
                if no_pos >= unwind_qty and self._should_sell(raw_yes, anchor, is_yes=False):
                    orders.extend(self._sell_orders(
                        market_id, adjusted_no_mid, spread,
                        False, unwind_qty))

        return orders


class FastAnchorMM(BaseAgent):
    """FBA-native spread-capture market maker with fast anchor.

    Key FBA insight: spread is an entry-selection filter, not a profit source.
    In FBA with minting, the MM accumulates positions at the clearing price.
    Wider spread → fills only when price has moved significantly → better
    mean-reversion entries. The MM profits when positions revert, not from
    the bid-ask spread directly.

    Design:
    - Fast EMA anchor (α=0.20) tracks current price closely (vs 0.03 in BalancedMM)
    - Wider base spread (3c vs 1c) as entry-quality filter
    - Symmetric quoting with inventory skew (no directional bias)
    - Vol-adaptive spread widening
    - Inventory selling when positions exceed threshold

    Backtest (36-day Iran sim): +$7,143 vs BalancedMM's -$2,767.
    Eliminates noise-trader exploit (noise PnL goes from +$3,394 to negative).
    """

    _BUY_TAPER = [0.50, 0.30, 0.20]
    _SELL_TAPER = [0.50, 0.30, 0.20]

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 50_000.0,
        half_spread: float = 0.03,
        num_levels: int = 3,
        level_spacing: float = 0.02,
        max_per_side_dollars: float = 100.0,
        max_position: int = 5_000,
        skew_factor: float = 0.15,
        vol_lookback: int = 4,
        vol_widen_max: float = 4.0,
        anchor_alpha: float = 0.20,
        inventory_sell_start: int = 50,
        name: str | None = None,
        market_ids: list[int] | None = None,
        max_blocks: int | None = None,
    ):
        super().__init__(client, account_id, name, market_ids, max_blocks=max_blocks)
        self.mm_budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.half_spread = half_spread
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_per_side = max_per_side_dollars
        self.max_position = max_position
        self.skew_factor = skew_factor
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.anchor_alpha = anchor_alpha
        self.inventory_sell_start = inventory_sell_start
        self._price_history: dict[int, list[float]] = {}
        self._anchor: dict[int, float] = {}

    def _update_anchor(self, market_id: int, price: float) -> float:
        """Update and return the fast anchor."""
        if market_id not in self._anchor:
            self._anchor[market_id] = price
            return price
        self._anchor[market_id] += self.anchor_alpha * (price - self._anchor[market_id])
        return self._anchor[market_id]

    def _vol_multiplier(self, market_id: int, mid: float) -> float:
        """Widen spread after large price moves."""
        history = self._price_history.setdefault(market_id, [])
        history.append(mid)
        if len(history) > self.vol_lookback + 2:
            del history[:-self.vol_lookback - 2]
        if len(history) < 2:
            return 1.0
        recent = history[-self.vol_lookback:]
        windowed_vol = max(recent) - min(recent)
        single_jump = abs(history[-1] - history[-2]) * 2.0
        effective_vol = max(windowed_vol, single_jump)
        if effective_vol > 0.02:
            return min(self.vol_widen_max, 1.0 + effective_vol * 10)
        return 1.0

    def _buy_orders(
        self, market_id: int, mid: float, spread: float,
        is_yes: bool, budget: float, scale: float = 1.0,
    ) -> list[OrderSpec]:
        """Generate tapered buy orders for one side."""
        orders = []
        cls = BuyYes if is_yes else BuyNo
        spent = 0.0
        for level in range(self.num_levels):
            bid = mid - spread - level * self.level_spacing
            if bid < 0.01:
                break
            w = self._BUY_TAPER[min(level, len(self._BUY_TAPER) - 1)]
            level_dollars = self.max_per_side * w * scale
            room = budget - spent
            if room <= 0:
                break
            level_dollars = min(level_dollars, room)
            qty = int(level_dollars / bid)
            if qty > 0:
                orders.append(cls.at_price(market_id, bid, qty))
                spent += qty * bid
        return orders

    def _sell_orders(
        self, market_id: int, mid: float, spread: float,
        is_yes: bool, held: int,
    ) -> list[OrderSpec]:
        """Generate tapered sell orders for one side."""
        orders = []
        cls = SellYes if is_yes else SellNo
        remaining = held
        for level in range(self.num_levels):
            ask = mid + spread + level * self.level_spacing
            if ask > 0.99 or remaining <= 0:
                break
            w = self._SELL_TAPER[min(level, len(self._SELL_TAPER) - 1)]
            qty = max(1, min(int(held * w), remaining))
            orders.append(cls.at_price(market_id, ask, qty))
            remaining -= qty
        return orders

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders: list[OrderSpec] = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            raw_yes = yes_nanos / NANOS_PER_DOLLAR
            raw_yes = max(0.05, min(0.95, raw_yes))
            anchor = self._update_anchor(market_id, raw_yes)

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            net = yes_pos - no_pos

            # Inventory skew: shift mid AWAY from net position to rebalance
            if self.max_position > 0:
                skew = -math.tanh(net / self.max_position) * self.skew_factor
            else:
                skew = 0.0
            yes_mid = max(0.05, min(0.95, anchor + skew))
            no_mid = 1.0 - yes_mid

            # Adaptive spread: base × volatility
            vol_mult = self._vol_multiplier(market_id, raw_yes)
            spread = self.half_spread * vol_mult

            # Compress near edges
            edge_room = min(yes_mid, no_mid)
            spread = min(spread, edge_room - 0.01)
            if spread < 0.005:
                continue

            # Dampen buying when total inventory is high
            total = yes_pos + no_pos
            if total > 100:
                buy_scale = max(0.1, 1.0 - min(1.0, total / (self.max_position * 2)) * 2.0)
            else:
                buy_scale = 1.0

            # Vol-inverse size scaling
            eff_per_side = self.max_per_side / vol_mult
            budget = min(self.current_balance, eff_per_side)

            # Symmetric buy orders on both sides
            if buy_scale > 0:
                orders.extend(self._buy_orders(
                    market_id, yes_mid, spread, True, budget, buy_scale))
                orders.extend(self._buy_orders(
                    market_id, no_mid, spread, False, budget, buy_scale))

            # Sell excess inventory on either side (no patience gating)
            for is_yes in [True, False]:
                pos = yes_pos if is_yes else no_pos
                if pos > self.inventory_sell_start:
                    mid = yes_mid if is_yes else no_mid
                    sell_frac = min(0.5, (pos - self.inventory_sell_start) / self.max_position * 2.0)
                    qty = max(1, int(pos * sell_frac))
                    orders.extend(self._sell_orders(
                        market_id, mid, spread, is_yes, qty))

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
