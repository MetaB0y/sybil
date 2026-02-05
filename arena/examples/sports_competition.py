#!/usr/bin/env python3
"""Sports-based competition using mock sports data.

This example creates markets based on (mock) sports games and runs
a trading competition. Use the real SportsDataFeed with an API key
for actual sports data.

Run:
    1. Start sybil-api: cargo run -p sybil-api -- --dev-mode --port 3001
    2. Run this script: python examples/sports_competition.py
"""

import asyncio
import sys

sys.path.insert(0, str(__file__).rsplit("/", 2)[0])

from rich.console import Console

from bots import (
    FixedProbabilityModel,
    InformedTrader,
    RandomTrader,
    SimpleMarketMaker,
)
from feeds import MockSportsDataFeed
from scripts.run_competition import (
    BotConfig,
    CompetitionConfig,
    run_full_competition,
)

console = Console()


async def main():
    console.print("[bold blue]Sports Prediction Market Competition[/bold blue]\n")

    # Generate mock sports data
    async with MockSportsDataFeed(seed=42) as feed:
        games = await feed.get_upcoming_games("basketball_nba")

        console.print("[bold]Today's Games:[/bold]")
        for game in games:
            console.print(f"  {game.home_team} vs {game.away_team}")
            console.print(f"    Home win odds: {game.home_odds:.1%}")
            console.print(f"    Away win odds: {game.away_odds:.1%}")

        # Create markets from games
        markets = []
        resolutions = {}
        true_probs = {}

        for i, game in enumerate(games[:2]):  # Use first 2 games
            market_specs = feed.game_to_markets(game)

            # Home team wins market
            home_market = market_specs[0]
            markets.append(home_market.name)
            true_probs[i * 2] = home_market.true_probability

            # Resolve based on mock outcome
            result = feed.resolve_game(game)
            resolutions[home_market.name] = 1.0 if result == "home" else 0.0

            # Away team wins market
            away_market = market_specs[1]
            markets.append(away_market.name)
            true_probs[i * 2 + 1] = away_market.true_probability
            resolutions[away_market.name] = 1.0 if result == "away" else 0.0

        console.print(f"\n[bold]Markets to trade: {len(markets)}[/bold]")
        for m in markets:
            console.print(f"  - {m}")

    # Competition config
    config = CompetitionConfig(
        name="Sports Betting Championship",
        initial_balance=100.0,
        duration_seconds=45,
        markets=markets,
        resolution_payouts=resolutions,
    )

    # Bots
    bot_configs = [
        # Market maker
        BotConfig(
            bot_class=SimpleMarketMaker,
            name="Bookmaker",
            kwargs={"spread_bps": 200, "quote_size": 5},
        ),
        # Sharp bettor who knows the true odds
        BotConfig(
            bot_class=InformedTrader,
            name="Sharp",
            kwargs={
                "model": FixedProbabilityModel(true_probs),
                "edge_threshold": 0.05,
                "order_size": 8,
            },
        ),
        # Casual bettors (noise)
        BotConfig(
            bot_class=RandomTrader,
            name="Casual-1",
            kwargs={"trade_probability": 0.5, "seed": 1},
        ),
        BotConfig(
            bot_class=RandomTrader,
            name="Casual-2",
            kwargs={"trade_probability": 0.4, "seed": 2},
        ),
    ]

    # Run
    result = await run_full_competition(
        base_url="http://localhost:3001",
        config=config,
        bot_configs=bot_configs,
        show_live=True,
    )

    # Show game outcomes
    console.print("\n[bold]Game Outcomes:[/bold]")
    for name, payout in resolutions.items():
        outcome = "YES" if payout > 0.5 else "NO"
        console.print(f"  {name}: {outcome}")


if __name__ == "__main__":
    asyncio.run(main())
