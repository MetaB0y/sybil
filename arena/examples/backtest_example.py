#!/usr/bin/env python3
"""Example backtest with news-based trading bots.

This example demonstrates running a backtest against historical NBA data.
Bots receive news (lineups, scores, injuries) and update their beliefs
to make trading decisions.

Run:
    1. Start sybil-api: cargo run --release -p sybil-api -- --dev-mode --port 3001
    2. Run this script: python examples/backtest_example.py

The backtest uses time compression (default 60x):
- 1 real second = 1 simulated minute
- A 5-hour dataset runs in ~5 real minutes
"""

import asyncio
import sys
from pathlib import Path

# Add parent directory for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from backtest import BacktestAgentConfig, BacktestRunner, Dataset
from bots import AggressiveNewsTrader, ConservativeNewsTrader, NewsTrader, SimpleMarketMaker


async def main():
    # Load the sample dataset
    dataset_path = Path(__file__).parent.parent / "datasets" / "nba_sample.json"
    dataset = Dataset.load(dataset_path)

    print(f"Loaded dataset: {dataset.name}")
    print(f"Events: {len(dataset.events)}")
    print(f"News items: {len(dataset.news)}")
    print(f"Duration: {dataset.duration / 3600:.1f} hours")
    print()

    # Configure agents
    # Note: We need to create a simple wrapper for non-BacktestAgent bots
    # For this example, we'll use only BacktestAgent-based bots
    agent_configs = [
        BacktestAgentConfig(
            agent_class=NewsTrader,
            name="NewsBot-Standard",
            kwargs={
                "edge_threshold": 0.05,
                "order_size": 5,
            },
        ),
        BacktestAgentConfig(
            agent_class=ConservativeNewsTrader,
            name="NewsBot-Conservative",
            kwargs={},
        ),
        BacktestAgentConfig(
            agent_class=AggressiveNewsTrader,
            name="NewsBot-Aggressive",
            kwargs={},
        ),
    ]

    # Create and run the backtest
    runner = BacktestRunner(
        base_url="http://localhost:3001",
        dataset=dataset,
        agent_configs=agent_configs,
        initial_balance=100.0,  # $100 per bot
        compression_ratio=60.0,  # 1 real second = 1 sim minute
    )

    result = await runner.run(show_live=True)

    # Additional analysis
    print("\n[Analysis]")
    print(f"Total real time: {result.duration_real_seconds:.1f} seconds")
    print(f"Simulated time: {result.duration_sim_seconds / 3600:.1f} hours")

    # Show market resolutions
    print("\n[Market Resolutions]")
    for event in dataset.events:
        market_id = result.market_ids.get(event.event_id)
        payout = result.resolutions.get(market_id, 0)
        outcome = "HOME" if payout == 1.0 else "AWAY" if payout == 0.0 else "DRAW"
        print(f"  {event.home_team} vs {event.away_team}: {outcome}")
        if event.final_score:
            print(f"    Final: {event.final_score.home} - {event.final_score.away}")


if __name__ == "__main__":
    asyncio.run(main())
