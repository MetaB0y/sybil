"""Shared market-price helpers for the live analyst/sizer split (SYB-210).

Both the :class:`~live.analyst.PersonaAnalyst` (which needs a price for its LLM
prompt) and the :class:`~live.trader.LiveLlmTrader` sizer (which needs a price
for mechanical rebalancing) resolve the reference price the same way. Factoring
it here keeps the two halves from drifting apart after the split — the AR-1
priority order (fresh Polymarket poll > startup reference snapshot > on-chain
clearing) lives in exactly one place.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from sybil_client.types import NANOS_PER_DOLLAR

if TYPE_CHECKING:
    from sybil_client import Block
    from sybil_client.types import Market

    from .news_feed import NewsFeed


def market_price(
    news_feed: "NewsFeed",
    markets_info: dict[int, "Market"],
    market_id: int,
    block: "Block",
) -> float:
    """Resolve the reference price for a market.

    AR-1: prefer the freshly polled Polymarket mid so the sizing engine sees the
    same price the LLM prompt is shown. The startup reference_price_nanos
    snapshot is only a fallback — it used to win here and froze sizing prices at
    process start. On-chain clearing is the last resort.
    """
    poly_price = news_feed.polymarket_prices.get_price(market_id)
    if poly_price and poly_price > 0:
        return poly_price

    market = markets_info.get(market_id)
    if market and market.reference_price_nanos is not None and market.reference_price_nanos > 0:
        return market.reference_price_nanos / NANOS_PER_DOLLAR

    if market_id in block.clearing_prices:
        yes_nanos, _ = block.clearing_prices[market_id]
        return yes_nanos / NANOS_PER_DOLLAR

    return 0.0


def observed_market_prices(
    news_feed: "NewsFeed",
    markets_info: dict[int, "Market"],
    market_ids: set[int] | None,
    block: "Block",
) -> dict[int, tuple[int, int]]:
    """Prices worth recording for live trading, without synthetic 50/50s."""
    ids = market_ids or set(block.clearing_prices.keys())
    prices: dict[int, tuple[int, int]] = {}
    for market_id in ids:
        ref = market_price(news_feed, markets_info, market_id, block)
        if ref > 0:
            yes_nanos = int(ref * NANOS_PER_DOLLAR)
            prices[market_id] = (yes_nanos, NANOS_PER_DOLLAR - yes_nanos)
    return prices
