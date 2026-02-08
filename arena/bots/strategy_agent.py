"""StrategyAgent — write one function to create a working trading bot.

Usage:
    def my_strategy(markets: dict[str, MarketView]) -> dict[str, float]:
        estimates = {}
        for name, view in markets.items():
            if any("injury" in n.lower() for n in view.news):
                estimates[name] = view.price * 0.8
        return estimates

    agent = StrategyAgent(..., strategy_fn=my_strategy)
"""

from dataclasses import dataclass, field
import logging

from sybil_client import Block, BuyNo, BuyYes, OrderSpec

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem

logger = logging.getLogger(__name__)

NANOS_PER_DOLLAR = 1_000_000_000


def format_news_line(news: NewsItem) -> str:
    """Format a news item as a concise one-liner.

    Shared between StrategyAgent and LLMNewsTrader.
    """
    meta = news.metadata
    if news.source == "in_game":
        quarter = meta.get("quarter", "?")
        home_score = meta.get("home_score", "?")
        away_score = meta.get("away_score", "?")
        if meta.get("final"):
            return f"[FINAL] {home_score} - {away_score}"
        return f"[Q{quarter} END] {home_score} - {away_score}"
    elif news.source == "injury":
        player = meta.get("player", "Unknown")
        status = meta.get("status", meta.get("severity", "unknown"))
        return f"[INJURY] {player} {status}"
    elif news.source == "lineup":
        return f"[LINEUP] {news.content[:80]}"
    else:
        return f"[{news.source.upper()}] {news.headline[:80]}"


@dataclass(frozen=True)
class MarketView:
    """Snapshot of a market visible to a strategy function."""

    name: str  # e.g. "Celtics vs Lakers"
    price: float  # current YES probability (0-1)
    news: list[str]  # formatted news lines, most recent first
    position: int  # net position (YES - NO)


class StrategyAgent(BacktestAgent):
    """A bot driven by a simple strategy function.

    The user provides a pure function:
        (markets: dict[str, MarketView]) -> dict[str, float]

    The function returns probability estimates keyed by display name.
    StrategyAgent handles order generation, position management,
    and mapping between display names and market IDs.
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        *,
        strategy_fn=None,
        edge_threshold: float = 0.05,
        order_size: int = 5,
        max_position: int = 50,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
        )
        if strategy_fn is None:
            raise ValueError("strategy_fn is required")
        self.strategy_fn = strategy_fn
        self.edge_threshold = edge_threshold
        self.order_size = order_size
        self.max_position = max_position

        # Per-event accumulated news (event_id -> list[str], newest first)
        self._event_news: dict[str, list[str]] = {}
        # event_id -> display name (e.g. "Celtics vs Lakers")
        self._event_display_names: dict[str, str] = {}
        # display_name -> market_id (for mapping strategy output back)
        self._display_to_market: dict[str, int] = {}
        # market_id -> display_name
        self._market_to_display: dict[int, str] = {}
        # market_id -> event_id
        self._market_to_event: dict[int, str] = {}

        if event_market_map:
            for event_id, market_id in event_market_map.items():
                self._market_to_event[market_id] = event_id
                # Default display name until we get team info from news
                self._event_display_names[event_id] = event_id

    async def on_news(self, news: NewsItem) -> None:
        """Accumulate formatted news per event, extract team names."""
        if news.event_id is None:
            return

        # Extract team names from metadata
        meta = news.metadata
        if "home_team" in meta and "away_team" in meta:
            home = meta["home_team"]
            away = meta["away_team"]
            # Build short display name
            display = f"{home} vs {away}"
            old_display = self._event_display_names.get(news.event_id)
            self._event_display_names[news.event_id] = display

            # Update mappings
            market_id = self.event_market_map.get(news.event_id)
            if market_id is not None:
                if old_display and old_display in self._display_to_market:
                    del self._display_to_market[old_display]
                self._display_to_market[display] = market_id
                self._market_to_display[market_id] = display

        # Accumulate formatted news (most recent first)
        formatted = format_news_line(news)
        if news.event_id not in self._event_news:
            self._event_news[news.event_id] = []
        self._event_news[news.event_id].insert(0, formatted)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        """Build MarketViews, call strategy_fn, convert estimates to orders."""
        # Ensure display mappings exist for all events
        for event_id, market_id in self.event_market_map.items():
            if market_id not in self._market_to_display:
                display = self._event_display_names.get(event_id, event_id)
                self._display_to_market[display] = market_id
                self._market_to_display[market_id] = display

        # Build MarketViews
        markets: dict[str, MarketView] = {}
        for market_id, (yes_nanos, _) in self.filter_markets(block).items():
            display = self._market_to_display.get(market_id)
            if display is None:
                continue
            event_id = self._market_to_event.get(market_id, "")
            news_lines = self._event_news.get(event_id, [])
            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")
            markets[display] = MarketView(
                name=display,
                price=yes_nanos / NANOS_PER_DOLLAR,
                news=list(news_lines),
                position=yes_pos - no_pos,
            )

        if not markets:
            return []

        # Call strategy function (wrapped in try/except)
        try:
            estimates = self.strategy_fn(markets)
        except Exception as e:
            logger.warning("[%s] Strategy function error: %s", self.name, e)
            return []

        if not isinstance(estimates, dict):
            return []

        # Sync estimates to beliefs for display visibility
        for display_name, prob in estimates.items():
            market_id = self._display_to_market.get(display_name)
            if market_id is not None:
                self.update_belief(market_id, prob)

        # Convert estimates to orders
        orders = []
        for display_name, prob in estimates.items():
            market_id = self._display_to_market.get(display_name)
            if market_id is None:
                continue

            prices = block.clearing_prices.get(market_id)
            if prices is None:
                continue

            market_prob = prices[0] / NANOS_PER_DOLLAR
            edge = prob - market_prob

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if edge > self.edge_threshold:
                if yes_pos < self.max_position:
                    bid_price = min(0.95, market_prob + 0.02)
                    orders.append(BuyYes.at_price(market_id, bid_price, self.order_size))
            elif edge < -self.edge_threshold:
                if no_pos < self.max_position:
                    no_price = 1 - market_prob
                    bid_price = min(0.95, no_price + 0.02)
                    orders.append(BuyNo.at_price(market_id, bid_price, self.order_size))

        return orders
