"""Sports data feed using The Odds API."""

import asyncio
from dataclasses import dataclass
from datetime import datetime
from typing import Any

import httpx


@dataclass
class Team:
    """A sports team."""

    name: str


@dataclass
class Game:
    """A sports game with two teams."""

    id: str
    sport: str
    home_team: str
    away_team: str
    commence_time: datetime
    home_odds: float | None = None  # Implied probability from odds
    away_odds: float | None = None
    draw_odds: float | None = None  # For soccer etc.


@dataclass
class MarketSpec:
    """Specification for a prediction market."""

    name: str
    true_probability: float | None = None  # If known (e.g., from odds)


class SportsDataFeed:
    """Fetch live sports odds and results from The Odds API.

    Sign up at https://the-odds-api.com/ for a free API key (500 requests/month).
    """

    BASE_URL = "https://api.the-odds-api.com/v4"

    # Available sports (subset)
    SPORTS = {
        "basketball_nba": "NBA Basketball",
        "basketball_ncaab": "NCAA Basketball",
        "americanfootball_nfl": "NFL Football",
        "americanfootball_ncaaf": "NCAA Football",
        "soccer_epl": "English Premier League",
        "soccer_usa_mls": "MLS",
        "baseball_mlb": "MLB Baseball",
        "icehockey_nhl": "NHL Hockey",
    }

    def __init__(self, api_key: str):
        """Initialize with API key.

        Args:
            api_key: API key from the-odds-api.com
        """
        self.api_key = api_key
        self._client: httpx.AsyncClient | None = None

    async def __aenter__(self) -> "SportsDataFeed":
        self._client = httpx.AsyncClient(timeout=30.0)
        return self

    async def __aexit__(self, *args: Any) -> None:
        if self._client:
            await self._client.aclose()

    @property
    def client(self) -> httpx.AsyncClient:
        if self._client is None:
            raise RuntimeError("Feed not initialized. Use 'async with SportsDataFeed():'")
        return self._client

    async def get_sports(self) -> list[dict[str, Any]]:
        """Get list of available sports."""
        response = await self.client.get(
            f"{self.BASE_URL}/sports",
            params={"apiKey": self.api_key},
        )
        response.raise_for_status()
        return response.json()

    async def get_upcoming_games(
        self, sport: str, regions: str = "us", markets: str = "h2h"
    ) -> list[Game]:
        """Get upcoming games with odds for a sport.

        Args:
            sport: Sport key (e.g., 'basketball_nba')
            regions: Odds region ('us', 'uk', 'eu', 'au')
            markets: Market type ('h2h' for moneyline, 'spreads', 'totals')

        Returns:
            List of games with odds
        """
        response = await self.client.get(
            f"{self.BASE_URL}/sports/{sport}/odds",
            params={
                "apiKey": self.api_key,
                "regions": regions,
                "markets": markets,
            },
        )
        response.raise_for_status()
        data = response.json()

        games = []
        for event in data:
            game = Game(
                id=event["id"],
                sport=sport,
                home_team=event["home_team"],
                away_team=event["away_team"],
                commence_time=datetime.fromisoformat(
                    event["commence_time"].replace("Z", "+00:00")
                ),
            )

            # Extract odds from first bookmaker
            if event.get("bookmakers"):
                bookmaker = event["bookmakers"][0]
                for market in bookmaker.get("markets", []):
                    if market["key"] == "h2h":
                        for outcome in market["outcomes"]:
                            prob = self._american_to_probability(outcome.get("price", 0))
                            if outcome["name"] == event["home_team"]:
                                game.home_odds = prob
                            elif outcome["name"] == event["away_team"]:
                                game.away_odds = prob
                            elif outcome["name"] == "Draw":
                                game.draw_odds = prob

            games.append(game)

        return games

    async def get_scores(self, sport: str, days_from: int = 1) -> list[dict[str, Any]]:
        """Get recent scores/results.

        Args:
            sport: Sport key
            days_from: Number of days back to fetch

        Returns:
            List of completed games with scores
        """
        response = await self.client.get(
            f"{self.BASE_URL}/sports/{sport}/scores",
            params={
                "apiKey": self.api_key,
                "daysFrom": days_from,
            },
        )
        response.raise_for_status()
        return response.json()

    def game_to_markets(self, game: Game) -> list[MarketSpec]:
        """Convert a game to prediction market specifications.

        Creates markets for:
        - Home team wins
        - Away team wins
        - Draw (if applicable)
        """
        markets = []

        # Home team wins
        markets.append(
            MarketSpec(
                name=f"{game.home_team} beats {game.away_team}",
                true_probability=game.home_odds,
            )
        )

        # Away team wins
        markets.append(
            MarketSpec(
                name=f"{game.away_team} beats {game.home_team}",
                true_probability=game.away_odds,
            )
        )

        # Draw (for soccer etc.)
        if game.draw_odds is not None:
            markets.append(
                MarketSpec(
                    name=f"{game.home_team} vs {game.away_team} ends in draw",
                    true_probability=game.draw_odds,
                )
            )

        return markets

    @staticmethod
    def _american_to_probability(american_odds: float) -> float:
        """Convert American odds to implied probability.

        American odds:
        - Positive (e.g., +150): Underdog, means $100 bet wins $150
        - Negative (e.g., -200): Favorite, means $200 bet wins $100
        """
        if american_odds == 0:
            return 0.5
        if american_odds > 0:
            return 100 / (american_odds + 100)
        else:
            return abs(american_odds) / (abs(american_odds) + 100)


class MockSportsDataFeed:
    """Mock sports feed for testing without API key."""

    def __init__(self, seed: int = 42):
        import random

        self.rng = random.Random(seed)
        self._games: list[Game] = []

    async def __aenter__(self) -> "MockSportsDataFeed":
        self._generate_games()
        return self

    async def __aexit__(self, *args: Any) -> None:
        pass

    def _generate_games(self) -> None:
        """Generate fake games."""
        teams = [
            ("Lakers", "Celtics"),
            ("Warriors", "Heat"),
            ("Bucks", "76ers"),
            ("Nuggets", "Suns"),
        ]

        for i, (home, away) in enumerate(teams):
            # Generate random odds that sum to ~1.05 (bookmaker margin)
            home_prob = self.rng.uniform(0.3, 0.7)
            away_prob = 1.0 - home_prob - 0.05  # 5% margin

            self._games.append(
                Game(
                    id=f"mock_{i}",
                    sport="basketball_nba",
                    home_team=home,
                    away_team=away,
                    commence_time=datetime.now(),
                    home_odds=home_prob,
                    away_odds=away_prob,
                )
            )

    async def get_upcoming_games(self, sport: str, **kwargs: Any) -> list[Game]:
        """Return mock games."""
        return [g for g in self._games if g.sport == sport]

    def game_to_markets(self, game: Game) -> list[MarketSpec]:
        """Same as real feed."""
        return SportsDataFeed.game_to_markets(SportsDataFeed, game)

    def resolve_game(self, game: Game) -> str:
        """Randomly resolve a game based on odds.

        Returns 'home', 'away', or 'draw'.
        """
        r = self.rng.random()
        if game.home_odds and r < game.home_odds:
            return "home"
        elif game.draw_odds and r < (game.home_odds or 0) + game.draw_odds:
            return "draw"
        else:
            return "away"
