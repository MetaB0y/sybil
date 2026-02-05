"""Random noise trader bot."""

import random

from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes

from .base import BaseAgent


class RandomTrader(BaseAgent):
    """Trades randomly to provide noise and liquidity.

    This bot randomly places orders with configurable probability and sizing.
    Only sells positions it actually owns. Buy orders can always be placed
    (and match via minting with opposing buy orders).
    """

    def __init__(
        self,
        client,
        account_id: int,
        trade_probability: float = 0.3,
        min_size: int = 1,
        max_size: int = 5,
        seed: int | None = None,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        """Initialize random trader.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            trade_probability: Probability of trading each block (0-1)
            min_size: Minimum order size
            max_size: Maximum order size
            seed: Random seed for reproducibility
            market_ids: Markets to trade (None = all)
        """
        super().__init__(client, account_id, name, market_ids)
        self.trade_probability = trade_probability
        self.min_size = min_size
        self.max_size = max_size
        self.rng = random.Random(seed)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        # Decide whether to trade this block
        if self.rng.random() > self.trade_probability:
            return []

        # Get available markets (filtered to our allowed markets)
        markets = self.filter_markets(block)
        if not markets:
            return []

        # Pick a random market
        market_id = self.rng.choice(list(markets.keys()))
        yes_nanos, no_nanos = markets[market_id]
        yes_price = yes_nanos / 1_000_000_000
        no_price = no_nanos / 1_000_000_000

        # Get positions
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")

        # Build list of valid actions
        actions = ["buy_yes", "buy_no"]  # Can always buy
        if yes_pos > 0:
            actions.append("sell_yes")
        if no_pos > 0:
            actions.append("sell_no")

        side = self.rng.choice(actions)

        # Random price: for buys, slightly below market; for sells, slightly above
        if side == "buy_yes":
            price = yes_price * self.rng.uniform(0.9, 1.05)
            size = self.rng.randint(self.min_size, self.max_size)
            return [BuyYes.at_price(market_id, max(0.01, min(0.99, price)), size)]
        elif side == "buy_no":
            price = no_price * self.rng.uniform(0.9, 1.05)
            size = self.rng.randint(self.min_size, self.max_size)
            return [BuyNo.at_price(market_id, max(0.01, min(0.99, price)), size)]
        elif side == "sell_yes":
            price = yes_price * self.rng.uniform(0.95, 1.1)
            size = min(yes_pos, self.rng.randint(self.min_size, self.max_size))
            return [SellYes.at_price(market_id, max(0.01, min(0.99, price)), size)]
        else:  # sell_no
            price = no_price * self.rng.uniform(0.95, 1.1)
            size = min(no_pos, self.rng.randint(self.min_size, self.max_size))
            return [SellNo.at_price(market_id, max(0.01, min(0.99, price)), size)]
