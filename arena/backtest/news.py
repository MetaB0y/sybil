"""News delivery scheduler for backtesting."""

import asyncio
from dataclasses import dataclass, field

from .clock import SimulatedClock
from .dataset import NewsItem


@dataclass
class NewsScheduler:
    """Delivers news items at correct simulated times.

    The scheduler holds a sorted list of news items and delivers them to
    subscribers via asyncio.Queue when their timestamp is reached according
    to the simulated clock.

    Usage:
        scheduler = NewsScheduler(clock, news_items)
        queue = scheduler.subscribe()

        # In a task:
        async for news in queue_iter(queue):
            print(news.headline)

        # Start delivery
        await scheduler.run()
    """

    clock: SimulatedClock
    news_items: list[NewsItem]
    _subscribers: list[asyncio.Queue[NewsItem]] = field(default_factory=list, init=False)
    _running: bool = field(default=False, init=False)
    _task: asyncio.Task | None = field(default=None, init=False)

    def __post_init__(self) -> None:
        # Sort news by timestamp
        self.news_items = sorted(self.news_items, key=lambda n: n.timestamp)
        self._delivered_count = 0

    def subscribe(self) -> asyncio.Queue[NewsItem]:
        """Subscribe to news delivery.

        Returns:
            An asyncio.Queue that will receive NewsItem objects as they're delivered.
        """
        queue: asyncio.Queue[NewsItem] = asyncio.Queue()
        self._subscribers.append(queue)
        return queue

    def unsubscribe(self, queue: asyncio.Queue[NewsItem]) -> None:
        """Unsubscribe from news delivery."""
        if queue in self._subscribers:
            self._subscribers.remove(queue)

    async def _deliver(self, news: NewsItem) -> None:
        """Deliver a news item to all subscribers."""
        for queue in self._subscribers:
            await queue.put(news)
        self._delivered_count += 1

    async def run(self) -> None:
        """Run the news scheduler until all news is delivered.

        This coroutine waits for each news item's timestamp and delivers
        it to all subscribers.
        """
        if self._running:
            return

        self._running = True
        self.clock.start()

        try:
            for news in self.news_items:
                if not self._running:
                    break

                # Wait until the news timestamp
                await self.clock.sleep_until(news.timestamp)

                if not self._running:
                    break

                # Deliver to all subscribers
                await self._deliver(news)

        finally:
            self._running = False

    def start(self) -> asyncio.Task:
        """Start the scheduler as a background task.

        Returns:
            The asyncio.Task running the scheduler.
        """
        self._task = asyncio.create_task(self.run())
        return self._task

    def stop(self) -> None:
        """Stop the scheduler."""
        self._running = False
        if self._task and not self._task.done():
            self._task.cancel()

    @property
    def is_running(self) -> bool:
        """Check if the scheduler is currently running."""
        return self._running

    @property
    def delivered_count(self) -> int:
        """Number of news items delivered so far."""
        return self._delivered_count

    @property
    def remaining_count(self) -> int:
        """Number of news items remaining to deliver."""
        return len(self.news_items) - self._delivered_count

    def get_upcoming(self, count: int = 5) -> list[NewsItem]:
        """Get the next N upcoming news items."""
        return self.news_items[self._delivered_count : self._delivered_count + count]


async def drain_queue(queue: asyncio.Queue[NewsItem]) -> list[NewsItem]:
    """Drain all items from a news queue without blocking.

    Returns:
        List of all news items currently in the queue.
    """
    items = []
    while True:
        try:
            item = queue.get_nowait()
            items.append(item)
        except asyncio.QueueEmpty:
            break
    return items
