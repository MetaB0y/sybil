"""Backtest agent base class with news support."""

import asyncio
from abc import abstractmethod
from dataclasses import dataclass, field
from typing import Any

from bots.base import BaseAgent
from sybil_client import Block, OrderSpec, SybilClient

from .clock import SimulatedClock
from .dataset import NewsItem
from .news import drain_queue


@dataclass
class Belief:
    """A bot's belief about a market outcome probability."""

    market_id: int
    probability: float  # 0-1, probability of YES outcome
    confidence: float = 1.0  # 0-1, how confident the bot is
    updated_at: float = 0.0  # simulated timestamp


class BacktestAgent(BaseAgent):
    """Base class for backtesting agents that receive news.

    Extends BaseAgent with:
    - News queue for receiving NewsItem objects
    - on_news() callback for processing news
    - Belief tracking for probability estimates
    - Access to simulated clock

    Subclasses should implement:
    - on_news(): Update beliefs when news arrives
    - on_block(): Make trading decisions based on beliefs

    Usage:
        class MyBot(BacktestAgent):
            async def on_news(self, news: NewsItem) -> None:
                # Update beliefs based on news
                if "injury" in news.headline.lower():
                    self.update_belief(market_id, new_prob, confidence=0.8)

            async def on_block(self, block: Block) -> list[OrderSpec]:
                # Trade based on beliefs
                orders = []
                for market_id, belief in self.beliefs.items():
                    market_price = block.clearing_prices.get(market_id)
                    if market_price and belief.probability > market_price[0] / 1e9 + 0.05:
                        orders.append(BuyYes.at_price(market_id, belief.probability, 5))
                return orders
    """

    def __init__(
        self,
        client: SybilClient,
        account_id: int,
        clock: SimulatedClock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
    ):
        """Initialize a backtest agent.

        Args:
            client: SybilClient instance
            account_id: Account to trade from
            clock: SimulatedClock for time reference
            name: Bot name
            market_ids: Markets to trade (None = all)
            event_market_map: Mapping from event_id to market_id
        """
        super().__init__(client, account_id, name, market_ids)
        self.clock = clock
        self.event_market_map = event_market_map or {}
        self.beliefs: dict[int, Belief] = {}
        self._news_queue: asyncio.Queue[NewsItem] | None = None
        self._news_task: asyncio.Task | None = None

    def set_news_queue(self, queue: asyncio.Queue[NewsItem]) -> None:
        """Set the news queue for this agent."""
        self._news_queue = queue

    @abstractmethod
    async def on_news(self, news: NewsItem) -> None:
        """Called when news arrives. Override to update beliefs.

        Args:
            news: The news item received.
        """
        pass

    def update_belief(
        self,
        market_id: int,
        probability: float,
        confidence: float = 1.0,
    ) -> None:
        """Update the agent's belief about a market.

        Args:
            market_id: The market to update belief for
            probability: New probability estimate (0-1)
            confidence: How confident in this estimate (0-1)
        """
        self.beliefs[market_id] = Belief(
            market_id=market_id,
            probability=max(0.0, min(1.0, probability)),
            confidence=max(0.0, min(1.0, confidence)),
            updated_at=self.clock.elapsed_sim_time().total_seconds(),
        )

    def get_belief(self, market_id: int) -> Belief | None:
        """Get the current belief for a market."""
        return self.beliefs.get(market_id)

    def get_market_for_event(self, event_id: str) -> int | None:
        """Get the market ID for an event."""
        return self.event_market_map.get(event_id)

    async def _process_news(self) -> None:
        """Background task to process incoming news."""
        if self._news_queue is None:
            return

        while self._running:
            try:
                # Wait for news with timeout to allow checking _running
                try:
                    news = await asyncio.wait_for(
                        self._news_queue.get(),
                        timeout=0.1,
                    )
                    await self.on_news(news)
                except asyncio.TimeoutError:
                    continue
            except asyncio.CancelledError:
                break
            except Exception as e:
                print(f"[{self.name}] Error processing news: {e}")

    async def run(self) -> None:
        """Main loop - process news and stream blocks."""
        self._running = True

        # Start news processing task
        if self._news_queue is not None:
            self._news_task = asyncio.create_task(self._process_news())

        try:
            async for block in self.client.stream_blocks():
                if not self._running:
                    break

                # Process any pending news first (drain queue)
                if self._news_queue:
                    pending_news = await drain_queue(self._news_queue)
                    for news in pending_news:
                        await self.on_news(news)

                # Update our state
                await self._update_state(block)

                # Get orders from strategy
                orders = await self.on_block(block)

                # Submit orders if any
                if orders:
                    self.last_orders = orders
                    self.total_orders_submitted += len(orders)
                    try:
                        await self.client.submit_orders(
                            self.account_id, orders,
                            mm_budget_nanos=self.mm_budget_nanos,
                        )
                    except Exception as e:
                        print(f"[{self.name}] Order submission failed: {e}")

        except Exception as e:
            print(f"[{self.name}] Error in run loop: {e}")
            raise
        finally:
            if self._news_task:
                self._news_task.cancel()
                try:
                    await self._news_task
                except asyncio.CancelledError:
                    pass

    def stop(self) -> None:
        """Stop the bot gracefully."""
        super().stop()
        if self._news_task and not self._news_task.done():
            self._news_task.cancel()


@dataclass
class BacktestAgentConfig:
    """Configuration for a backtest agent."""

    agent_class: type[BacktestAgent]
    name: str
    kwargs: dict[str, Any] = field(default_factory=dict)
