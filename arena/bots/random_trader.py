"""Random noise trader bot."""

import math
import random

from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes

from .base import BaseAgent


class RandomTrader(BaseAgent):
    """Trades randomly to provide noise and liquidity.

    Buys use up to 50% of available USDC. Sells use up to 100% of position.
    Prices are noised around the last clearing price.
    """

    def __init__(
        self,
        client,
        account_id: int,
        trade_probability: float = 0.5,
        price_noise: float = 0.035,
        seed: int | None = None,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name, market_ids)
        self.trade_probability = trade_probability
        self.price_noise = price_noise
        self.rng = random.Random(seed)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        if self.rng.random() > self.trade_probability:
            return []

        markets = self.filter_markets(block)
        if not markets:
            return []

        market_id = self.rng.choice(list(markets.keys()))
        yes_nanos, no_nanos = markets[market_id]
        yes_price = yes_nanos / 1_000_000_000
        no_price = no_nanos / 1_000_000_000

        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        usdc = self.current_balance

        # Build valid actions
        actions = []
        if usdc > 0.01:
            actions += ["buy_yes", "buy_no"]
        if yes_pos > 0:
            actions.append("sell_yes")
        if no_pos > 0:
            actions.append("sell_no")
        if not actions:
            return []

        side = self.rng.choice(actions)
        noise = 1 - self.price_noise, 1 + self.price_noise

        if side == "buy_yes":
            price = yes_price * self.rng.uniform(*noise)
            price = max(0.01, min(0.99, price))
            max_shares = math.floor(0.5 * usdc / price)
            if max_shares < 1:
                return []
            size = self.rng.randint(1, max_shares)
            return [BuyYes.at_price(market_id, price, size)]
        elif side == "buy_no":
            price = no_price * self.rng.uniform(*noise)
            price = max(0.01, min(0.99, price))
            max_shares = math.floor(0.5 * usdc / price)
            if max_shares < 1:
                return []
            size = self.rng.randint(1, max_shares)
            return [BuyNo.at_price(market_id, price, size)]
        elif side == "sell_yes":
            price = yes_price * self.rng.uniform(*noise)
            price = max(0.01, min(0.99, price))
            size = self.rng.randint(1, yes_pos)
            return [SellYes.at_price(market_id, price, size)]
        else:
            price = no_price * self.rng.uniform(*noise)
            price = max(0.01, min(0.99, price))
            size = self.rng.randint(1, no_pos)
            return [SellNo.at_price(market_id, price, size)]
