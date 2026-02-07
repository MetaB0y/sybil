"""Flash liquidity market maker using Sybil's MM budget constraints.

Unlike SimpleMarketMaker which is limited by per-order balance checks,
FlashMarketMaker submits orders with mm_budget_nanos, allowing ~20x
capital efficiency. The solver picks the welfare-optimal subset that
fits within the budget.

MM orders are one-shot (not persisted across blocks), so the MM must
re-quote every block.
"""

from sybil_client import Block, BuyNo, BuyYes, OrderSpec

from .base import BaseAgent

NANOS_PER_DOLLAR = 1_000_000_000


class FlashMarketMaker(BaseAgent):
    """Market maker using flash liquidity (portfolio-level budget constraint).

    Submits buy orders on both YES and NO sides at multiple price levels
    for each market. The total notional can far exceed the MM's balance
    because the solver only activates the subset that fits the budget.

    Key differences from SimpleMarketMaker:
    - Submits orders via mm_budget_nanos (skip per-order balance checks)
    - Quotes at multiple price levels (depth)
    - Adjusts quotes based on inventory skew
    - All orders are one-shot (re-quoted every block)
    """

    def __init__(
        self,
        client,
        account_id: int,
        budget_dollars: float = 1000.0,
        half_spread_bps: int = 100,
        num_levels: int = 3,
        level_spacing_bps: int = 50,
        quote_size: int = 10,
        skew_factor: float = 0.1,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        """Initialize flash market maker.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            budget_dollars: Total capital budget for the MM
            half_spread_bps: Half-spread in basis points (100 = 1%)
            num_levels: Number of price levels to quote on each side
            level_spacing_bps: Spacing between levels in bps
            quote_size: Size per quote level
            skew_factor: How much to adjust mid based on inventory (0-1)
            name: Bot name
            market_ids: Markets to trade (None = all)
        """
        super().__init__(client, account_id, name, market_ids)
        self.budget_nanos = int(budget_dollars * NANOS_PER_DOLLAR)
        self.half_spread_bps = half_spread_bps
        self.num_levels = num_levels
        self.level_spacing_bps = level_spacing_bps
        self.quote_size = quote_size
        self.skew_factor = skew_factor

    def _compute_skew(self, market_id: int) -> float:
        """Compute inventory skew adjustment.

        Returns a value to add to the mid price. Positive = we're long YES
        so we want to sell YES (shift mid up to make YES asks more attractive).
        """
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        net = yes_pos - no_pos
        return net * self.skew_factor * 0.01  # Scale down

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            # Current mid price
            yes_mid = yes_nanos / NANOS_PER_DOLLAR
            no_mid = no_nanos / NANOS_PER_DOLLAR

            # Apply inventory skew
            skew = self._compute_skew(market_id)
            yes_mid = max(0.05, min(0.95, yes_mid + skew))
            no_mid = max(0.05, min(0.95, 1.0 - yes_mid))

            half_spread = self.half_spread_bps / 10000
            level_spacing = self.level_spacing_bps / 10000

            # Quote multiple levels on each side
            for level in range(self.num_levels):
                offset = half_spread + level * level_spacing

                # YES side: bid below mid, offer above mid
                yes_bid = max(0.01, yes_mid - offset)
                # NO side: bid below mid
                no_bid = max(0.01, no_mid - offset)

                # Buy YES at bid
                orders.append(BuyYes.at_price(market_id, yes_bid, self.quote_size))
                # Buy NO at bid
                orders.append(BuyNo.at_price(market_id, no_bid, self.quote_size))

        # Submit with MM budget constraint
        if orders:
            try:
                await self.client.submit_orders(
                    self.account_id,
                    orders,
                    mm_budget_nanos=self.budget_nanos,
                )
            except Exception as e:
                print(f"[{self.name}] MM order submission failed: {e}")

        # Return empty - we already submitted with mm_budget
        return []


class TightFlashMM(FlashMarketMaker):
    """Tight-spread flash MM. Profits from volume, vulnerable to adverse selection."""

    def __init__(
        self,
        client,
        account_id: int,
        name: str | None = None,
        market_ids: list[int] | None = None,
        budget_dollars: float = 1000.0,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            budget_dollars=budget_dollars,
            half_spread_bps=50,     # 0.5% half-spread (1% total)
            num_levels=4,           # More depth
            level_spacing_bps=25,   # Tight spacing
            quote_size=15,          # Larger size
            skew_factor=0.15,       # More aggressive skew
            name=name,
            market_ids=market_ids,
        )


class WideFlashMM(FlashMarketMaker):
    """Wide-spread flash MM. Less volume but survives adverse selection better."""

    def __init__(
        self,
        client,
        account_id: int,
        name: str | None = None,
        market_ids: list[int] | None = None,
        budget_dollars: float = 1000.0,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            budget_dollars=budget_dollars,
            half_spread_bps=200,    # 2% half-spread (4% total)
            num_levels=2,           # Less depth
            level_spacing_bps=100,  # Wide spacing
            quote_size=8,           # Smaller size
            skew_factor=0.05,       # Less aggressive skew
            name=name,
            market_ids=market_ids,
        )
