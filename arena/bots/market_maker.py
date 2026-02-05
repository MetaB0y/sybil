"""Simple market maker bot."""

from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes

from .base import BaseAgent


class SimpleMarketMaker(BaseAgent):
    """Quotes both sides of each market with a configurable spread.

    This bot provides liquidity by placing buy orders on both YES and NO.
    In prediction markets, BuyYes + BuyNo can match via minting (total cost = $1).
    The MM profits from the bid-ask spread when prices move.
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
        """Initialize market maker.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            spread_bps: Spread in basis points (100 bps = 1%)
            quote_size: Size to quote on each side
            max_position: Maximum position on one side
            market_ids: Markets to trade (None = all)
        """
        super().__init__(client, account_id, name, market_ids)
        self.spread_bps = spread_bps
        self.quote_size = quote_size
        self.max_position = max_position

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            # Calculate prices with spread
            yes_price = yes_nanos / 1_000_000_000
            no_price = no_nanos / 1_000_000_000
            half_spread = (self.spread_bps / 10000) / 2

            # Bid for YES (below market) and NO (below market)
            yes_bid = max(0.01, yes_price - half_spread)
            no_bid = max(0.01, no_price - half_spread)

            # Get current positions
            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            # Buy YES if not too long
            if yes_pos < self.max_position:
                orders.append(BuyYes.at_price(market_id, yes_bid, self.quote_size))

            # Buy NO if not too long
            if no_pos < self.max_position:
                orders.append(BuyNo.at_price(market_id, no_bid, self.quote_size))

            # Sell positions we have (to take profit or reduce exposure)
            if yes_pos > 0:
                ask_price = min(0.99, yes_price + half_spread)
                sell_qty = min(yes_pos, self.quote_size)
                orders.append(SellYes.at_price(market_id, ask_price, sell_qty))

            if no_pos > 0:
                ask_price = min(0.99, no_price + half_spread)
                sell_qty = min(no_pos, self.quote_size)
                orders.append(SellNo.at_price(market_id, ask_price, sell_qty))

        return orders
