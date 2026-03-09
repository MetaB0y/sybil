"""News-based trading bot for backtesting."""

from sybil_client import Block, BuyNo, BuyYes, OrderSpec

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem


class NewsTrader(BacktestAgent):
    """A simple bot that updates beliefs based on news and trades accordingly.

    This bot demonstrates how to use the BacktestAgent pattern:
    - Processes news via on_news() to update beliefs
    - Makes trading decisions in on_block() based on beliefs vs market prices

    Strategy:
    - Starts with prior belief of 0.5 for each market
    - Adjusts beliefs based on score updates (leader gets higher probability)
    - Adjusts beliefs on injury news (injured team gets lower probability)
    - Trades when belief differs from market price by > edge_threshold
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        edge_threshold: float = 0.05,
        order_size: int = 5,
        max_position: int = 50,
    ):
        """Initialize the news trader.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            clock: SimulatedClock instance
            name: Bot name
            market_ids: Markets to trade
            event_market_map: Mapping from event_id to market_id
            edge_threshold: Minimum edge to trade (default 5%)
            order_size: Size of each order
            max_position: Maximum position per side
        """
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
        )
        self.edge_threshold = edge_threshold
        self.order_size = order_size
        self.max_position = max_position

        # Initialize beliefs to 0.5 for all markets
        if market_ids:
            for mid in market_ids:
                self.update_belief(mid, 0.5, confidence=0.5)

    async def on_news(self, news: NewsItem) -> None:
        """Update beliefs when news arrives.

        Args:
            news: The news item received.
        """
        market_id = self.get_market_for_event(news.event_id)
        if market_id is None:
            return

        current_belief = self.get_belief(market_id)
        current_prob = current_belief.probability if current_belief else 0.5

        # Process based on news type
        if news.source == "in_game":
            # Score updates - adjust belief based on who's winning
            home_score = news.metadata.get("home_score", 0)
            away_score = news.metadata.get("away_score", 0)

            if home_score + away_score > 0:
                # Simple model: probability proportional to score ratio
                # with some dampening towards 0.5
                score_ratio = home_score / (home_score + away_score)
                # Blend with prior, more weight as game progresses
                quarter = news.metadata.get("quarter", 1)
                game_progress = min(1.0, quarter / 4)
                new_prob = (1 - game_progress * 0.7) * 0.5 + (game_progress * 0.7) * score_ratio

                self.update_belief(market_id, new_prob, confidence=0.3 + game_progress * 0.5)

        elif news.source == "injury":
            # Injury news - major injuries hurt the team
            severity = news.metadata.get("severity", "")
            status = news.metadata.get("status", "")

            adjustment = 0.0
            if status == "out" or severity == "serious":
                adjustment = -0.15  # Big negative impact
            elif severity == "questionable":
                adjustment = -0.08  # Moderate negative

            # Determine which team is affected
            # For simplicity, check if home team player is mentioned in headline
            # In real implementation, you'd have team rosters
            headline_lower = news.headline.lower()

            # This is simplified - in reality you'd know which team the player is on
            # For now, assume injury to featured player hurts home team
            new_prob = current_prob + adjustment
            self.update_belief(market_id, new_prob, confidence=0.7)

        elif news.source == "lineup":
            # Lineup news - could adjust based on key players
            # For simplicity, just increase confidence in current belief
            self.update_belief(market_id, current_prob, confidence=0.6)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        """Make trading decisions based on beliefs vs market prices.

        Args:
            block: The current block with market prices.

        Returns:
            List of orders to submit.
        """
        orders = []

        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            belief = self.get_belief(market_id)
            if belief is None:
                continue

            # Get market probability (from YES price)
            market_prob = yes_nanos / 1_000_000_000

            # Calculate edge (weighted by confidence)
            raw_edge = belief.probability - market_prob
            weighted_edge = raw_edge * belief.confidence

            # Get current positions
            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if weighted_edge > self.edge_threshold:
                # We think YES is underpriced - buy YES
                if yes_pos < self.max_position:
                    bid_price = min(0.95, market_prob + 0.02)
                    orders.append(BuyYes.at_price(market_id, bid_price, self.order_size))

            elif weighted_edge < -self.edge_threshold:
                # We think YES is overpriced - buy NO
                if no_pos < self.max_position:
                    no_price = 1 - market_prob
                    bid_price = min(0.95, no_price + 0.02)
                    orders.append(BuyNo.at_price(market_id, bid_price, self.order_size))

        return orders


class ConservativeNewsTrader(NewsTrader):
    """A more conservative news trader that requires higher edge.

    Same strategy as NewsTrader but with higher thresholds and smaller sizes.
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
            edge_threshold=0.10,  # Higher threshold
            order_size=3,  # Smaller size
            max_position=30,  # Lower max
        )


class AggressiveNewsTrader(NewsTrader):
    """An aggressive news trader with lower edge requirements.

    Same strategy as NewsTrader but trades more frequently.
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
            edge_threshold=0.03,  # Lower threshold
            order_size=8,  # Larger size
            max_position=80,  # Higher max
        )
