#!/usr/bin/env python3
"""Full competition example with multiple bot types.

This example demonstrates running a complete trading competition with:
- Market makers (different spread widths)
- Informed traders (with knowledge of true probabilities)
- Momentum traders (following price trends)
- Random noise traders

Run:
    1. Start sybil-api: cargo run -p sybil-api -- --dev-mode --port 3001
    2. Run this script: python examples/full_competition.py
"""

import asyncio
import sys

# Add parent directory for imports
sys.path.insert(0, str(__file__).rsplit("/", 2)[0])

from bots import (
    FixedProbabilityModel,
    InformedTrader,
    MomentumTrader,
    RandomTrader,
    SimpleMarketMaker,
)
from scripts.run_competition import (
    BotConfig,
    CompetitionConfig,
    run_full_competition,
)


async def main():
    # Competition configuration
    config = CompetitionConfig(
        name="AI Trading Championship",
        initial_balance=100.0,  # $100 per bot
        duration_seconds=60,  # 1 minute
        markets=[
            "Will BTC reach $150k this year?",
            "Will ETH flip BTC market cap?",
            "Will AI pass Turing test by 2027?",
        ],
        resolution_payouts={
            # Markets resolve at these probabilities
            "Will BTC reach $150k this year?": 0.65,  # 65% YES
            "Will ETH flip BTC market cap?": 0.20,  # 20% YES
            "Will AI pass Turing test by 2027?": 0.80,  # 80% YES
        },
    )

    # Bot configurations
    bot_configs = [
        # Market makers with different spreads
        BotConfig(
            bot_class=SimpleMarketMaker,
            name="MM-Tight",
            kwargs={"spread_bps": 100, "quote_size": 3},  # 1% spread
        ),
        BotConfig(
            bot_class=SimpleMarketMaker,
            name="MM-Wide",
            kwargs={"spread_bps": 300, "quote_size": 5},  # 3% spread
        ),
        # Informed trader who "knows" the true probabilities
        # Uses market indices (0,1,2) mapping to competition markets in order
        BotConfig(
            bot_class=InformedTrader,
            name="Oracle",
            kwargs={
                "model": FixedProbabilityModel(
                    {
                        0: 0.65,  # First market (BTC) - matches resolution
                        1: 0.20,  # Second market (ETH)
                        2: 0.80,  # Third market (AI)
                    }
                ),
                "edge_threshold": 0.03,
                "order_size": 5,
                "use_market_index": True,  # Model uses indices, not absolute IDs
            },
        ),
        # Momentum trader
        BotConfig(
            bot_class=MomentumTrader,
            name="Momentum",
            kwargs={"lookback": 5, "momentum_threshold": 0.02},
        ),
        # Random traders as noise
        BotConfig(
            bot_class=RandomTrader,
            name="Noise-1",
            kwargs={"trade_probability": 0.4, "seed": 42},
        ),
        BotConfig(
            bot_class=RandomTrader,
            name="Noise-2",
            kwargs={"trade_probability": 0.3, "seed": 123},
        ),
    ]

    # Run competition
    result = await run_full_competition(
        base_url="http://localhost:3001",
        config=config,
        bot_configs=bot_configs,
        show_live=True,
    )

    # Additional analysis
    print("\n[Analysis]")
    winner = result.leaderboard()[0]
    print(f"Winner strategy: {winner.name}")

    # Check if informed trader (Oracle) won as expected
    oracle_result = next((r for r in result.bot_results if r.name == "Oracle"), None)
    if oracle_result:
        rank = result.leaderboard().index(oracle_result) + 1
        print(f"Informed trader (Oracle) finished #{rank} with ${oracle_result.pnl:+.2f}")


if __name__ == "__main__":
    asyncio.run(main())
