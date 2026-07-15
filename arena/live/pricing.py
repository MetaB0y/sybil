"""Shared market-price helpers for the live analyst/sizer split (SYB-210).

Both the :class:`~live.analyst.PersonaAnalyst` (which needs a price for its LLM
prompt) and the :class:`~live.trader.LiveLlmTrader` sizer (which needs a price
for mechanical rebalancing) resolve the reference price the same way. The API
owns external-price age and supplies an exact expiry: reference-required runs
stop when it is absent, while other runs may fall back to local clearing.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from sybil_client.types import NANOS_PER_DOLLAR

if TYPE_CHECKING:
    from sybil_client import Block

    from .news_feed import NewsFeed


def market_price(
    news_feed: "NewsFeed",
    market_id: int,
    block: "Block",
) -> float:
    """Resolve the reference price for a market.

    Prefer the API-bounded external reference so the sizing engine sees the same
    price the LLM prompt is shown. A reference-required run fails closed when it
    is unavailable. Other runs may use on-chain clearing as the last resort.
    """
    reference_price = news_feed.reference_prices.get_price(market_id)
    if reference_price and reference_price > 0:
        return reference_price
    if news_feed.require_reference_prices:
        return 0.0

    if market_id in block.clearing_prices:
        yes_nanos, _ = block.clearing_prices[market_id]
        return yes_nanos / NANOS_PER_DOLLAR

    return 0.0


def observed_market_prices(
    news_feed: "NewsFeed",
    market_ids: set[int] | None,
    block: "Block",
) -> dict[int, tuple[int, int]]:
    """Prices worth recording for live trading, without synthetic 50/50s."""
    ids = market_ids or set(block.clearing_prices.keys())
    prices: dict[int, tuple[int, int]] = {}
    for market_id in ids:
        ref = market_price(news_feed, market_id, block)
        if ref > 0:
            yes_nanos = int(ref * NANOS_PER_DOLLAR)
            prices[market_id] = (yes_nanos, NANOS_PER_DOLLAR - yes_nanos)
    return prices
