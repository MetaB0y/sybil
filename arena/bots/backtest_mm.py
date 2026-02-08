"""Backtest-compatible flash liquidity market maker.

Wraps the same MM quoting logic as FlashMarketMaker but extends BacktestAgent
so it works with BacktestRunner's news scheduling and simulated clock.
"""

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem
from sybil_client import Block, BuyNo, BuyYes, OrderSpec

NANOS_PER_DOLLAR = 1_000_000_000


class BacktestFlashMM(BacktestAgent):
    """Flash market maker for backtesting.

    Submits buy orders on both YES and NO sides at multiple price levels,
    using mm_budget_nanos for capital-efficient quoting. Re-quotes every block.
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        budget_dollars: float = 1000.0,
        half_spread_bps: int = 100,
        num_levels: int = 3,
        level_spacing_bps: int = 50,
        quote_size: int = 10,
        skew_factor: float = 0.1,
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
        self.half_spread_bps = half_spread_bps
        self.num_levels = num_levels
        self.level_spacing_bps = level_spacing_bps
        self.quote_size = quote_size
        self.skew_factor = skew_factor

    async def on_news(self, news: NewsItem) -> None:
        """MMs don't trade on news."""
        pass

    def _compute_skew(self, market_id: int) -> float:
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        net = yes_pos - no_pos
        return net * self.skew_factor * 0.01

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            yes_mid = yes_nanos / NANOS_PER_DOLLAR
            skew = self._compute_skew(market_id)
            yes_mid = max(0.05, min(0.95, yes_mid - skew))
            no_mid = max(0.05, min(0.95, 1.0 - yes_mid))

            half_spread = self.half_spread_bps / 10000
            level_spacing = self.level_spacing_bps / 10000

            for level in range(self.num_levels):
                offset = half_spread + level * level_spacing
                yes_bid = max(0.01, yes_mid - offset)
                no_bid = max(0.01, no_mid - offset)
                orders.append(BuyYes.at_price(market_id, yes_bid, self.quote_size))
                orders.append(BuyNo.at_price(market_id, no_bid, self.quote_size))

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
    """Tight-spread backtest MM. Profits from volume, vulnerable to adverse selection."""

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
            half_spread_bps=50,
            num_levels=4,
            level_spacing_bps=25,
            quote_size=15,
            skew_factor=0.15,
        )


class BacktestWideMM(BacktestFlashMM):
    """Wide-spread backtest MM. Less volume but survives adverse selection better."""

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
            half_spread_bps=200,
            num_levels=2,
            level_spacing_bps=100,
            quote_size=8,
            skew_factor=0.05,
        )
