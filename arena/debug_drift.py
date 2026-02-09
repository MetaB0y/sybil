"""Debug script: trace per-market price evolution with only MMs.

Runs TightMM + WideMM on a few markets with NO directional bots.
Prints detailed per-block, per-market info to find the drift source.

Usage:
    # Terminal 1: cargo run --release -p sybil-api -- --dev-mode --port 3001
    # Terminal 2: cd arena && uv run python debug_drift.py
"""

import asyncio
import json
from sybil_client import SybilClient, BuyYes, BuyNo

NANOS = 1_000_000_000


class DebugMM:
    """Minimal MM that submits symmetric BuyYes + BuyNo orders."""

    def __init__(self, name, client, account_id, market_ids,
                 half_spread_bps=50, num_levels=4, spacing_bps=25,
                 quote_size=15, budget_dollars=1000.0):
        self.name = name
        self.client = client
        self.account_id = account_id
        self.market_ids = market_ids
        self.half_spread = half_spread_bps / 10000
        self.num_levels = num_levels
        self.spacing = spacing_bps / 10000
        self.quote_size = quote_size
        self.budget_nanos = int(budget_dollars * NANOS)

    async def submit(self, clearing_prices):
        orders = []
        for mid in self.market_ids:
            prices = clearing_prices.get(mid)
            if not prices:
                yes_mid = 0.5
            else:
                yes_mid = prices[0] / NANOS
            no_mid = 1.0 - yes_mid
            yes_mid = max(0.05, min(0.95, yes_mid))
            no_mid = max(0.05, min(0.95, no_mid))

            for level in range(self.num_levels):
                offset = self.half_spread + level * self.spacing
                yes_bid = max(0.01, yes_mid - offset)
                no_bid = max(0.01, no_mid - offset)
                orders.append(BuyYes.at_price(mid, yes_bid, self.quote_size))
                orders.append(BuyNo.at_price(mid, no_bid, self.quote_size))

        if orders:
            await self.client.submit_orders(
                self.account_id, orders, mm_budget_nanos=self.budget_nanos
            )
        return orders


async def main():
    base_url = "http://localhost:3001"

    async with SybilClient(base_url) as client:
        health = await client.health()
        print(f"Connected. Block height: {health.get('height', 0)}")

        # Create 3 markets
        markets = []
        for i in range(3):
            m = await client.create_market(f"Debug Market {i}")
            markets.append(m.id)
            print(f"Created market #{m.id}")

        # Create accounts
        tight_acct = await client.create_account(int(1000 * NANOS))
        wide_acct = await client.create_account(int(1000 * NANOS))

        tight_mm = DebugMM("Tight", client, tight_acct.id, markets,
                           half_spread_bps=50, num_levels=4, spacing_bps=25,
                           quote_size=15)
        wide_mm = DebugMM("Wide", client, wide_acct.id, markets,
                          half_spread_bps=200, num_levels=2, spacing_bps=100,
                          quote_size=8)

        print(f"\nTracking {len(markets)} markets for 30 blocks...\n")
        print(f"{'Block':>6}  {'Market':>8}  {'YES%':>6}  {'NO%':>6}  {'Fills':>5}  {'Detail'}")
        print("-" * 70)

        block_count = 0
        async for block in client.stream_blocks():
            # Submit MM orders on every block
            tight_orders = await tight_mm.submit(block.clearing_prices)
            wide_orders = await wide_mm.submit(block.clearing_prices)

            # Print per-market info
            for mid in markets:
                prices = block.clearing_prices.get(mid)
                if not prices:
                    continue
                yes_p = prices[0] / NANOS * 100
                no_p = prices[1] / NANOS * 100

                # Count fills for this market
                # (We don't have per-market fill info directly, but we can check fills)
                market_fills = 0  # Can't determine from block fills without order mapping

                detail = ""
                if abs(yes_p - 50.0) > 0.01:
                    detail = f"DRIFT! +{yes_p - 50:.2f}pp"

                print(f"{block.height:>6}  M{mid:>7}  {yes_p:>5.2f}  {no_p:>5.2f}  {block.orders_filled:>5}  {detail}")

            block_count += 1
            if block_count >= 30:
                break

        # Check final account balances
        tight_final = await client.get_account(tight_acct.id)
        wide_final = await client.get_account(wide_acct.id)
        print(f"\nTight MM balance: ${tight_final.balance_dollars:.4f} (started $1000)")
        print(f"Wide MM balance: ${wide_final.balance_dollars:.4f} (started $1000)")


if __name__ == "__main__":
    asyncio.run(main())
