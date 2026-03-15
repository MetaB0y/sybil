#!/usr/bin/env python3
"""Collect historical sports data from the-odds-api for backtesting.

This script fetches completed games and generates news templates that can
be manually enriched or used as-is for backtesting.

Usage:
    python scripts/collect_sports_data.py \\
        --api-key YOUR_API_KEY \\
        --sport basketball_nba \\
        --days 3 \\
        --output datasets/nba_sample.json

Available sports:
    - basketball_nba
    - basketball_ncaab
    - americanfootball_nfl
    - americanfootball_ncaaf
    - baseball_mlb
    - icehockey_nhl
    - soccer_epl
    - soccer_usa_mls

API Key: Get one at https://the-odds-api.com/
"""

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

import httpx

from backtest import Dataset, Event, FinalScore, MarketSpec, NewsItem


ODDS_API_BASE = "https://api.the-odds-api.com/v4"


def fetch_completed_events(
    api_key: str,
    sport: str,
    days_back: int = 3,
) -> list[dict]:
    """Fetch completed events from the-odds-api.

    Args:
        api_key: API key for the-odds-api
        sport: Sport key (e.g., 'basketball_nba')
        days_back: How many days back to look

    Returns:
        List of event dictionaries from the API
    """
    # Calculate date range
    end_date = datetime.now(timezone.utc)
    start_date = end_date - timedelta(days=days_back)

    url = f"{ODDS_API_BASE}/sports/{sport}/scores/"
    params = {
        "apiKey": api_key,
        "daysFrom": days_back,
        "dateFormat": "iso",
    }

    response = httpx.get(url, params=params, timeout=30.0)
    response.raise_for_status()

    events = response.json()

    # Filter to only completed events with scores
    completed = [
        e for e in events
        if e.get("completed", False) and e.get("scores")
    ]

    return completed


def generate_news_for_event(event: dict, commence_time: datetime, end_time: datetime) -> list[NewsItem]:
    """Generate template news items for an event.

    This creates basic news items that can be enriched with real data:
    - Pre-game lineup announcement
    - Quarter/period scores (for basketball/hockey)
    - Final score

    Args:
        event: Event dictionary from the API
        commence_time: When the game starts
        end_time: When the game ends

    Returns:
        List of NewsItem objects
    """
    news = []
    home_team = event["home_team"]
    away_team = event["away_team"]

    # Get scores
    scores = {s["name"]: int(s["score"]) for s in event.get("scores", [])}
    home_score = scores.get(home_team, 0)
    away_score = scores.get(away_team, 0)

    # Pre-game lineup (30 minutes before)
    news.append(NewsItem(
        timestamp=commence_time - timedelta(minutes=30),
        headline=f"Starting lineups announced: {home_team} vs {away_team}",
        content=f"Both teams have announced their starting lineups for tonight's game. "
                f"{home_team} will host {away_team} at their home arena.",
        source="lineup",
        event_id=event["id"],
        metadata={"home_team": home_team, "away_team": away_team},
    ))

    # In-game updates (simulate quarter scores for basketball)
    game_duration = (end_time - commence_time).total_seconds()

    # Generate 4 quarter updates for basketball
    for quarter in range(1, 5):
        quarter_time = commence_time + timedelta(seconds=game_duration * quarter / 4)

        # Interpolate scores (simple linear)
        q_home = int(home_score * quarter / 4)
        q_away = int(away_score * quarter / 4)

        # Add some variance
        if quarter < 4:
            q_home = max(0, q_home + (quarter % 2) * 3 - 2)
            q_away = max(0, q_away - (quarter % 2) * 2 + 1)

        if quarter == 4:
            q_home = home_score
            q_away = away_score

        leader = home_team if q_home > q_away else away_team if q_away > q_home else "Tied"

        news.append(NewsItem(
            timestamp=quarter_time,
            headline=f"End of Q{quarter}: {home_team} {q_home} - {away_team} {q_away}",
            content="It's a close game!" if abs(q_home - q_away) <= 5 else f"{leader} leads.",
            source="in_game",
            event_id=event["id"],
            metadata={
                "quarter": quarter,
                "home_score": q_home,
                "away_score": q_away,
            },
        ))

    # Final result (at end time)
    winner = home_team if home_score > away_score else away_team
    news.append(NewsItem(
        timestamp=end_time,
        headline=f"Final: {home_team} {home_score} - {away_team} {away_score}",
        content=f"{winner} wins! Final score: {home_team} {home_score}, {away_team} {away_score}.",
        source="in_game",
        event_id=event["id"],
        metadata={
            "final": True,
            "home_score": home_score,
            "away_score": away_score,
            "winner": winner,
        },
    ))

    return news


def api_event_to_dataset_event(event: dict) -> tuple[Event, list[NewsItem]]:
    """Convert an API event to our Event format.

    Args:
        event: Event dictionary from the-odds-api

    Returns:
        Tuple of (Event, list of NewsItem)
    """
    home_team = event["home_team"]
    away_team = event["away_team"]

    # Parse commence time
    commence_str = event["commence_time"]
    commence_time = datetime.fromisoformat(commence_str.replace("Z", "+00:00"))

    # Estimate end time (NBA games ~2.5 hours)
    end_time = commence_time + timedelta(hours=2, minutes=30)

    # Get scores
    scores = {s["name"]: int(s["score"]) for s in event.get("scores", [])}
    home_score = scores.get(home_team, 0)
    away_score = scores.get(away_team, 0)

    # Determine outcome
    if home_score > away_score:
        outcome = "home"
    elif away_score > home_score:
        outcome = "away"
    else:
        outcome = "draw"

    dataset_event = Event(
        event_id=event["id"],
        home_team=home_team,
        away_team=away_team,
        commence_time=commence_time,
        end_time=end_time,
        actual_outcome=outcome,
        final_score=FinalScore(home=home_score, away=away_score),
        markets=[MarketSpec(market_name=f"{home_team} beats {away_team}")],
    )

    news = generate_news_for_event(event, commence_time, end_time)

    return dataset_event, news


def create_dataset(
    api_key: str,
    sport: str,
    days_back: int = 3,
    name: str | None = None,
) -> Dataset:
    """Create a backtesting dataset from the-odds-api data.

    Args:
        api_key: API key for the-odds-api
        sport: Sport key (e.g., 'basketball_nba')
        days_back: How many days back to fetch
        name: Dataset name (defaults to sport + date range)

    Returns:
        Dataset object ready for backtesting
    """
    print(f"Fetching {sport} events from past {days_back} days...")
    api_events = fetch_completed_events(api_key, sport, days_back)
    print(f"Found {len(api_events)} completed events")

    if not api_events:
        raise ValueError(f"No completed events found for {sport} in the past {days_back} days")

    events = []
    all_news = []

    for api_event in api_events:
        try:
            event, news = api_event_to_dataset_event(api_event)
            events.append(event)
            all_news.extend(news)
            print(f"  Added: {event.home_team} vs {event.away_team}")
        except Exception as e:
            print(f"  Skipped {api_event.get('id', 'unknown')}: {e}")

    if not events:
        raise ValueError("No events could be processed")

    # Sort events by commence time
    events.sort(key=lambda e: e.commence_time)
    all_news.sort(key=lambda n: n.timestamp)

    # Calculate time range
    time_start = min(n.timestamp for n in all_news)
    time_end = max(e.end_time for e in events)

    # Generate name
    if name is None:
        sport_name = sport.replace("_", " ").title()
        date_str = events[0].commence_time.strftime("%Y-%m-%d")
        name = f"{sport_name} {date_str}"

    return Dataset(
        name=name,
        sport=sport,
        time_range=(time_start, time_end),
        events=events,
        news=all_news,
    )


def main():
    parser = argparse.ArgumentParser(
        description="Collect historical sports data for backtesting",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "--api-key",
        required=True,
        help="API key for the-odds-api.com",
    )
    parser.add_argument(
        "--sport",
        default="basketball_nba",
        help="Sport key (default: basketball_nba)",
    )
    parser.add_argument(
        "--days",
        type=int,
        default=3,
        help="Days of history to fetch (default: 3)",
    )
    parser.add_argument(
        "--output",
        default="datasets/sports_data.json",
        help="Output file path (default: datasets/sports_data.json)",
    )
    parser.add_argument(
        "--name",
        help="Dataset name (default: auto-generated)",
    )

    args = parser.parse_args()

    # Create dataset
    dataset = create_dataset(
        api_key=args.api_key,
        sport=args.sport,
        days_back=args.days,
        name=args.name,
    )

    # Save
    output_path = Path(args.output)
    dataset.save(output_path)
    print(f"\nSaved dataset to {output_path}")
    print(f"  Events: {len(dataset.events)}")
    print(f"  News items: {len(dataset.news)}")
    print(f"  Duration: {dataset.duration / 3600:.1f} hours")


if __name__ == "__main__":
    main()
