"""Dataset schema for backtesting historical sports events."""

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Literal
import json
from pathlib import Path


@dataclass
class MarketSpec:
    """Specification for a market within an event."""

    market_name: str
    market_type: Literal["moneyline", "spread", "total"] = "moneyline"

    def to_dict(self) -> dict[str, Any]:
        return {
            "market_name": self.market_name,
            "market_type": self.market_type,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "MarketSpec":
        return cls(
            market_name=data["market_name"],
            market_type=data.get("market_type", "moneyline"),
        )


@dataclass
class NewsItem:
    """A news item delivered during the backtest."""

    timestamp: datetime
    headline: str
    content: str
    source: Literal["lineup", "injury", "in_game", "weather", "other"]
    event_id: str | None = None
    metadata: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "timestamp": self.timestamp.isoformat(),
            "headline": self.headline,
            "content": self.content,
            "source": self.source,
            "event_id": self.event_id,
            "metadata": self.metadata,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "NewsItem":
        return cls(
            timestamp=datetime.fromisoformat(data["timestamp"]),
            headline=data["headline"],
            content=data["content"],
            source=data["source"],
            event_id=data.get("event_id"),
            metadata=data.get("metadata", {}),
        )


@dataclass
class FinalScore:
    """Final score of a completed event."""

    home: int
    away: int

    def to_dict(self) -> dict[str, int]:
        return {"home": self.home, "away": self.away}

    @classmethod
    def from_dict(cls, data: dict[str, int]) -> "FinalScore":
        return cls(home=data["home"], away=data["away"])


@dataclass
class Event:
    """A sports event (game) in the dataset."""

    event_id: str
    home_team: str
    away_team: str
    commence_time: datetime
    end_time: datetime
    actual_outcome: Literal["home", "away", "draw"]
    final_score: FinalScore | None = None
    markets: list[MarketSpec] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "event_id": self.event_id,
            "home_team": self.home_team,
            "away_team": self.away_team,
            "commence_time": self.commence_time.isoformat(),
            "end_time": self.end_time.isoformat(),
            "actual_outcome": self.actual_outcome,
            "final_score": self.final_score.to_dict() if self.final_score else None,
            "markets": [m.to_dict() for m in self.markets],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "Event":
        return cls(
            event_id=data["event_id"],
            home_team=data["home_team"],
            away_team=data["away_team"],
            commence_time=datetime.fromisoformat(data["commence_time"]),
            end_time=datetime.fromisoformat(data["end_time"]),
            actual_outcome=data["actual_outcome"],
            final_score=FinalScore.from_dict(data["final_score"])
            if data.get("final_score")
            else None,
            markets=[MarketSpec.from_dict(m) for m in data.get("markets", [])],
        )

    @property
    def moneyline_market_name(self) -> str:
        """Default market name for this event's moneyline."""
        return f"{self.home_team} beats {self.away_team}"


@dataclass
class Dataset:
    """A dataset of historical sports events for backtesting."""

    name: str
    sport: str
    time_range: tuple[datetime, datetime]
    events: list[Event]
    news: list[NewsItem]

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "sport": self.sport,
            "time_range": [self.time_range[0].isoformat(), self.time_range[1].isoformat()],
            "events": [e.to_dict() for e in self.events],
            "news": [n.to_dict() for n in self.news],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "Dataset":
        time_range = (
            datetime.fromisoformat(data["time_range"][0]),
            datetime.fromisoformat(data["time_range"][1]),
        )
        return cls(
            name=data["name"],
            sport=data["sport"],
            time_range=time_range,
            events=[Event.from_dict(e) for e in data["events"]],
            news=[NewsItem.from_dict(n) for n in data["news"]],
        )

    def save(self, path: str | Path) -> None:
        """Save dataset to JSON file."""
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w") as f:
            json.dump(self.to_dict(), f, indent=2)

    @classmethod
    def load(cls, path: str | Path) -> "Dataset":
        """Load dataset from JSON file."""
        with open(path) as f:
            data = json.load(f)
        return cls.from_dict(data)

    def get_news_for_event(self, event_id: str) -> list[NewsItem]:
        """Get all news items for a specific event."""
        return [n for n in self.news if n.event_id == event_id]

    def get_news_in_range(
        self, start: datetime, end: datetime
    ) -> list[NewsItem]:
        """Get news items within a time range, sorted by timestamp."""
        filtered = [n for n in self.news if start <= n.timestamp <= end]
        return sorted(filtered, key=lambda n: n.timestamp)

    @property
    def duration(self) -> float:
        """Total duration of the dataset in seconds."""
        return (self.time_range[1] - self.time_range[0]).total_seconds()
