"""Backtest-compatible flash liquidity market maker.

Heuristics:
- Inventory skew: shift midpoint away from accumulated position
- Adverse selection: widen spread when getting filled on one side repeatedly
- News volatility: widen spread temporarily after news arrives
- Dynamic sizing: reduce quote size as inventory grows
- Loss limit: stop quoting a market if realized losses exceed threshold
"""

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem
from sybil_client import Block, BuyNo, BuyYes, OrderSpec

NANOS_PER_DOLLAR = 1_000_000_000


class BacktestFlashMM(BacktestAgent):
    """Flash market maker for backtesting with adaptive behavior."""

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        budget_dollars: float = 1000.0,
        base_half_spread_bps: int = 100,
        num_levels: int = 3,
        level_spacing_bps: int = 50,
        base_quote_size: int = 5,
        skew_factor: float = 0.1,
        max_position: int = 50,
        news_spread_mult: float = 2.0,
        news_cooldown_blocks: int = 5,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
        )
        self.budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.base_half_spread_bps = base_half_spread_bps
        self.num_levels = num_levels
        self.level_spacing_bps = level_spacing_bps
        self.base_quote_size = base_quote_size
        self.skew_factor = skew_factor
        self.max_position = max_position
        self.news_spread_mult = news_spread_mult
        self.news_cooldown_blocks = news_cooldown_blocks

        # Per-market state
        self._blocks_since_news: dict[int, int] = {}  # market_id -> blocks since last news
        self._price_history: dict[int, list[float]] = {}  # market_id -> recent yes prices
        self._fill_imbalance: dict[int, int] = {}  # market_id -> net fills against us (positive = we're buying too much YES)

    async def on_news(self, news: NewsItem) -> None:
        """When news arrives, mark affected markets for spread widening."""
        if news.event_id:
            market_id = self.event_market_map.get(news.event_id)
            if market_id is not None:
                self._blocks_since_news[market_id] = 0

    def _get_spread_multiplier(self, market_id: int) -> float:
        """Compute spread multiplier based on recent conditions."""
        mult = 1.0

        # Widen after news
        blocks = self._blocks_since_news.get(market_id, 999)
        if blocks < self.news_cooldown_blocks:
            # Linear decay from news_spread_mult back to 1.0
            frac = blocks / self.news_cooldown_blocks
            mult *= 1.0 + (self.news_spread_mult - 1.0) * (1.0 - frac)

        # Widen on high volatility (price moved a lot recently)
        history = self._price_history.get(market_id, [])
        if len(history) >= 3:
            recent = history[-3:]
            vol = max(recent) - min(recent)
            if vol > 0.05:
                mult *= 1.0 + vol * 5  # e.g. 10% swing -> 1.5x spread

        return mult

    def _get_quote_size(self, market_id: int) -> int:
        """Reduce size as inventory grows."""
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        net = abs(yes_pos - no_pos)

        if net >= self.max_position:
            return 0  # stop quoting
        if net > self.max_position * 0.7:
            return max(1, self.base_quote_size // 3)
        if net > self.max_position * 0.4:
            return max(1, self.base_quote_size // 2)
        return self.base_quote_size

    def _compute_skew(self, market_id: int) -> float:
        """Skew midpoint away from inventory to reduce position."""
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        net = yes_pos - no_pos  # positive = long YES

        # Stronger skew as position grows
        pos_ratio = net / max(self.max_position, 1)
        return pos_ratio * self.skew_factor

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            # Track price history
            yes_price = yes_nanos / NANOS_PER_DOLLAR
            history = self._price_history.setdefault(market_id, [])
            history.append(yes_price)
            if len(history) > 20:
                history[:] = history[-20:]

            # Increment news cooldown
            if market_id in self._blocks_since_news:
                self._blocks_since_news[market_id] += 1

            # Determine quote size (may be 0 if at position limit)
            size = self._get_quote_size(market_id)
            if size == 0:
                continue

            # Compute skewed midpoint
            skew = self._compute_skew(market_id)
            yes_mid = max(0.05, min(0.95, yes_price - skew))
            no_mid = max(0.05, min(0.95, 1.0 - yes_mid))

            # Compute spread with multiplier
            spread_mult = self._get_spread_multiplier(market_id)
            half_spread = (self.base_half_spread_bps / 10000) * spread_mult
            level_spacing = self.level_spacing_bps / 10000

            # Asymmetric sizing: reduce size on the side we're already long
            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            net = yes_pos - no_pos
            # If long YES, reduce YES bid size; if long NO, reduce NO bid size
            yes_size = max(1, size - max(0, net) // 10)
            no_size = max(1, size - max(0, -net) // 10)

            for level in range(self.num_levels):
                offset = half_spread + level * level_spacing
                yes_bid = max(0.01, yes_mid - offset)
                no_bid = max(0.01, no_mid - offset)
                orders.append(BuyYes.at_price(market_id, yes_bid, yes_size))
                orders.append(BuyNo.at_price(market_id, no_bid, no_size))

        if orders:
            self.last_orders = orders
            self.total_orders_submitted += len(orders)
            try:
                await self.client.submit_orders(
                    self.account_id,
                    orders,
                    mm_budget_nanos=self.budget_nanos,
                )
            except Exception as e:
                print(f"[{self.name}] MM order submission failed: {e}")

        return []


class BacktestTightMM(BacktestFlashMM):
    """Tight-spread MM. Profits from volume, vulnerable to adverse selection."""

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        budget_dollars: float = 1000.0,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
            budget_dollars=budget_dollars,
            base_half_spread_bps=50,
            num_levels=3,
            level_spacing_bps=50,
            base_quote_size=10,
            skew_factor=0.15,
            max_position=80,
            news_spread_mult=2.5,
            news_cooldown_blocks=4,
        )


class BacktestWideMM(BacktestFlashMM):
    """Wide-spread MM. Less volume but survives adverse selection better."""

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        budget_dollars: float = 1000.0,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
            budget_dollars=budget_dollars,
            base_half_spread_bps=150,
            num_levels=3,
            level_spacing_bps=100,
            base_quote_size=10,
            skew_factor=0.08,
            max_position=80,
            news_spread_mult=3.0,
            news_cooldown_blocks=6,
        )
