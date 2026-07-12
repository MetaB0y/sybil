"""Per-persona fair-value broadcast bus (SYB-210).

Splits analysis from sizing. A single :class:`~live.analyst.PersonaAnalyst`
runs the analysis LLM once per drained article batch and publishes a
:class:`FairValueUpdate` onto its persona's :class:`FairValueBus`; the persona's
two sizing arms (Kelly, Flat) each subscribe and destructively drain their own
private copy. Because both arms consume the SAME update object, their A/B inputs
are provably identical — and the analysis LLM is called N times (once per
persona) instead of 2N (once per sizer).

This is the mechanical analogue of ``NewsSubscription`` in
:mod:`live.news_feed`: a bounded, drop-oldest, per-subscriber destructive-drain
broadcast where a late subscriber only ever sees updates published after it
joined. The update objects are shared by reference across subscribers and are
treated as read-only.
"""

from __future__ import annotations

import asyncio
import logging
from collections import defaultdict, deque
from dataclasses import dataclass, field
from datetime import datetime
from hashlib import sha256

from .news_feed import LiveArticle

log = logging.getLogger(__name__)

# Per-subscriber, per-market queue bound (mirrors news_feed.MAX_SUBSCRIBER_QUEUE).
# A stalled sizer must not grow memory without limit, so its oldest queued
# updates are dropped once this many pile up for a single market.
MAX_SUBSCRIBER_QUEUE = 500


def analysis_batch_id(
    market_id: int,
    articles: list[LiveArticle],
    reference_price: float | None = None,
) -> str:
    """Domain-separated id for one market, article URL batch, and price context."""
    urls = sorted(article.url for article in articles)
    price = "none" if reference_price is None else float(reference_price).hex()
    material = "\0".join(("sybil/analysis-batch/v2", str(market_id), price, *urls))
    return sha256(material.encode("utf-8")).hexdigest()


@dataclass
class FairValueUpdate:
    """One persona's fair-value estimate for one market from one LLM call."""

    market_id: int
    persona_key: str
    fair_value: float
    motivation: str
    analysis: str
    restate: str = ""
    countercase: str = ""
    confidence: float | None = None
    articles: list[LiveArticle] = field(default_factory=list)
    block_height: int = 0
    ts: datetime | None = None
    analysis_reference_price: float | None = None
    analysis_batch_id: str = ""

    def __post_init__(self) -> None:
        if not self.analysis_batch_id:
            self.analysis_batch_id = analysis_batch_id(
                self.market_id,
                self.articles,
                self.analysis_reference_price,
            )


class FairValueSubscription:
    """A single sizer's private, destructive view of a persona's FairValueBus.

    Draining one subscriber's queue does not consume another subscriber's copy,
    so both sizing arms of a persona observe identical fair values. Each
    per-market queue is bounded (drop-oldest, with a warning).
    """

    def __init__(
        self,
        bus: "FairValueBus",
        max_queue: int = MAX_SUBSCRIBER_QUEUE,
        name: str | None = None,
    ):
        self._bus = bus
        self._max_queue = max_queue
        self.name = name
        self._pending: dict[int, deque[FairValueUpdate]] = defaultdict(deque)

    def _deliver(self, market_id: int, update: FairValueUpdate) -> None:
        """Enqueue an update for one market. Caller must hold the bus lock."""
        queue = self._pending[market_id]
        queue.append(update)
        while len(queue) > self._max_queue:
            dropped = queue.popleft()
            log.warning(
                "Sizer %s fair-value queue for market %d full (max=%d); dropping "
                "oldest update (fv=%.2f)",
                self.name or id(self),
                market_id,
                self._max_queue,
                dropped.fair_value,
            )

    async def drain(self, market_id: int) -> list[FairValueUpdate]:
        """Destructively pop this subscriber's pending updates for a market."""
        async with self._bus._lock:
            queue = self._pending.get(market_id)
            if not queue:
                return []
            updates = list(queue)
            queue.clear()
        return updates


class FairValueBus:
    """Broadcast bus for one persona's fair-value updates.

    The runner creates one bus per persona: the persona's analyst publishes onto
    it and the persona's two sizers subscribe to it. Delivery fans out (each
    subscriber gets its own copy); the lock guards both delivery and drain.
    """

    def __init__(self, persona_key: str | None = None):
        self.persona_key = persona_key
        self._subscribers: list[FairValueSubscription] = []
        self._lock = asyncio.Lock()

    def subscribe(
        self,
        max_queue: int = MAX_SUBSCRIBER_QUEUE,
        name: str | None = None,
    ) -> FairValueSubscription:
        """Register a subscriber and return its private, drainable view.

        Call once per sizer so both arms of a persona observe identical fair
        values. A late subscriber only sees updates published after it joins.
        """
        sub = FairValueSubscription(self, max_queue=max_queue, name=name)
        self._subscribers.append(sub)
        return sub

    def unsubscribe(self, sub: FairValueSubscription) -> None:
        """Stop delivering to a subscriber. Idempotent."""
        try:
            self._subscribers.remove(sub)
        except ValueError:
            pass

    async def publish(self, update: FairValueUpdate) -> None:
        """Broadcast one update to every subscriber's per-market queue."""
        async with self._lock:
            for sub in self._subscribers:
                sub._deliver(update.market_id, update)
