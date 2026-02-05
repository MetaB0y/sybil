"""Informed trader bot that trades on edge between model and market."""

from abc import ABC, abstractmethod

from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes

from .base import BaseAgent


class ProbabilityModel(ABC):
    """Abstract base class for probability models."""

    @abstractmethod
    def get_probability(self, market_id: int) -> float | None:
        """Return model's probability for YES outcome.

        Returns None if model has no opinion on this market.
        """
        pass


class FixedProbabilityModel(ProbabilityModel):
    """Model with fixed probabilities per market."""

    def __init__(self, probabilities: dict[int, float]):
        """Initialize with fixed probability map.

        Args:
            probabilities: Dict of market_id -> probability (0-1)
        """
        self.probabilities = probabilities

    def get_probability(self, market_id: int) -> float | None:
        return self.probabilities.get(market_id)


class InformedTrader(BaseAgent):
    """Trades when model probability differs from market price.

    This bot compares its internal model's probability estimate to the
    market price and trades when there's sufficient edge. Positive edge
    (model > market) means buy YES, negative edge means buy NO.
    """

    def __init__(
        self,
        client,
        account_id: int,
        model: ProbabilityModel,
        edge_threshold: float = 0.05,
        order_size: int = 5,
        max_position: int = 50,
        name: str | None = None,
        market_ids: list[int] | None = None,
        use_market_index: bool = False,
    ):
        """Initialize informed trader.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            model: Probability model to use for trading decisions
            edge_threshold: Minimum edge required to trade (0-1)
            order_size: Size of each order
            max_position: Maximum position on one side
            market_ids: Markets to trade (None = all)
            use_market_index: If True, model uses indices (0,1,2...) mapping to market_ids order
        """
        super().__init__(client, account_id, name, market_ids)
        self.model = model
        self.edge_threshold = edge_threshold
        self.order_size = order_size
        self.max_position = max_position
        self.use_market_index = use_market_index
        # Build index mapping if using indices
        self._market_to_index = {}
        if use_market_index and market_ids:
            self._market_to_index = {mid: idx for idx, mid in enumerate(market_ids)}

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            # Get market probability (from YES price)
            market_prob = yes_nanos / 1_000_000_000

            # Get model probability - use index if configured
            if self.use_market_index:
                idx = self._market_to_index.get(market_id)
                model_prob = self.model.get_probability(idx) if idx is not None else None
            else:
                model_prob = self.model.get_probability(market_id)
            if model_prob is None:
                continue

            # Calculate edge
            edge = model_prob - market_prob

            # Get current positions
            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if edge > self.edge_threshold:
                # Model says YES is underpriced - buy YES
                if yes_pos < self.max_position:
                    # Bid slightly above market to get filled
                    bid_price = min(0.99, market_prob + 0.01)
                    orders.append(BuyYes.at_price(market_id, bid_price, self.order_size))

                # If we have NO position, sell it
                if no_pos > 0:
                    ask_price = max(0.01, (1 - market_prob) - 0.01)
                    sell_qty = min(no_pos, self.order_size)
                    orders.append(SellNo.at_price(market_id, ask_price, sell_qty))

            elif edge < -self.edge_threshold:
                # Model says YES is overpriced - buy NO (short YES)
                if no_pos < self.max_position:
                    # Bid slightly above market for NO
                    no_price = 1 - market_prob
                    bid_price = min(0.99, no_price + 0.01)
                    orders.append(BuyNo.at_price(market_id, bid_price, self.order_size))

                # If we have YES position, sell it
                if yes_pos > 0:
                    ask_price = max(0.01, market_prob - 0.01)
                    sell_qty = min(yes_pos, self.order_size)
                    orders.append(SellYes.at_price(market_id, ask_price, sell_qty))

        return orders


class MomentumTrader(BaseAgent):
    """Trades based on recent price momentum.

    Buys when prices are rising, sells when falling.
    A simple technical analysis strategy.
    """

    def __init__(
        self,
        client,
        account_id: int,
        lookback: int = 5,
        momentum_threshold: float = 0.02,
        order_size: int = 5,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        """Initialize momentum trader.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            lookback: Number of blocks to look back for momentum
            momentum_threshold: Minimum price change to trigger trade
            order_size: Size of each order
            market_ids: Markets to trade (None = all)
        """
        super().__init__(client, account_id, name, market_ids)
        self.lookback = lookback
        self.momentum_threshold = momentum_threshold
        self.order_size = order_size
        self.price_history: dict[int, list[float]] = {}  # market_id -> recent prices

    async def on_block(self, block: Block) -> list[OrderSpec]:
        orders = []

        for market_id, (yes_nanos, _) in self.filter_markets(block).items():
            current_price = yes_nanos / 1_000_000_000

            # Update price history
            if market_id not in self.price_history:
                self.price_history[market_id] = []
            self.price_history[market_id].append(current_price)

            # Keep only recent prices
            if len(self.price_history[market_id]) > self.lookback:
                self.price_history[market_id] = self.price_history[market_id][-self.lookback:]

            # Need enough history
            if len(self.price_history[market_id]) < self.lookback:
                continue

            # Calculate momentum (simple: current vs average)
            avg_price = sum(self.price_history[market_id][:-1]) / (self.lookback - 1)
            momentum = current_price - avg_price

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if momentum > self.momentum_threshold:
                # Price rising - buy YES
                orders.append(BuyYes.at_price(market_id, min(0.99, current_price + 0.01), self.order_size))
                # Sell NO if we have it
                if no_pos > 0:
                    orders.append(SellNo.at_price(market_id, max(0.01, (1 - current_price) - 0.02), min(no_pos, self.order_size)))

            elif momentum < -self.momentum_threshold:
                # Price falling - buy NO
                orders.append(BuyNo.at_price(market_id, min(0.99, (1 - current_price) + 0.01), self.order_size))
                # Sell YES if we have it
                if yes_pos > 0:
                    orders.append(SellYes.at_price(market_id, max(0.01, current_price - 0.02), min(yes_pos, self.order_size)))

        return orders
