"""Periodic portfolio rebalancer for LLM traders."""

import asyncio
import logging

from .clock import SimulatedClock

log = logging.getLogger(__name__)


async def run_rebalancer(traders, clock: SimulatedClock, client, interval_hours: float = 4):
    """Periodically trigger rebalancing for all traders with positions.

    Checks every hour but each trader's should_rebalance() enforces the actual interval.
    Runs concurrently across traders within a single pause window.
    """
    while True:
        await clock.sleep_sim(3600)  # check every sim hour

        to_rebalance = [t for t in traders if t.should_rebalance(interval_hours)]
        if not to_rebalance:
            continue

        print(f"\n  -- REBALANCE: {len(to_rebalance)} traders --", flush=True)

        # Get latest block for price context
        block = await client.get_latest_block()

        # Single pause for entire batch
        await client.pause()
        clock.pause()
        try:
            results = await asyncio.gather(
                *[t.rebalance(block) for t in to_rebalance],
                return_exceptions=True,
            )
        finally:
            clock.resume()
            await client.resume()

        # Submit returned orders
        for trader, result in zip(to_rebalance, results):
            if isinstance(result, BaseException):
                log.warning("[%s] Rebalance error: %s", trader.name, result)
                continue
            if result:
                await client.submit_orders(trader.account_id, result)
