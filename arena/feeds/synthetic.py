"""Synthetic data feed for testing."""

import random
from dataclasses import dataclass
from datetime import datetime


@dataclass
class SyntheticEvent:
    """A synthetic event that can be bet on."""

    id: str
    name: str
    true_probability: float
    resolution_time: datetime


class SyntheticFeed:
    """Generates synthetic events for testing.

    Use this when you don't have access to real data feeds
    or want deterministic test scenarios.
    """

    def __init__(self, seed: int = 42):
        self.rng = random.Random(seed)
        self._event_counter = 0

    def generate_event(self, name: str | None = None, true_prob: float | None = None) -> SyntheticEvent:
        """Generate a single synthetic event.

        Args:
            name: Optional event name
            true_prob: True probability (0-1), random if not provided
        """
        self._event_counter += 1
        event_id = f"synthetic_{self._event_counter}"

        if name is None:
            name = f"Synthetic Event #{self._event_counter}"

        if true_prob is None:
            true_prob = self.rng.random()

        return SyntheticEvent(
            id=event_id,
            name=name,
            true_probability=true_prob,
            resolution_time=datetime.now(),
        )

    def generate_events(self, count: int) -> list[SyntheticEvent]:
        """Generate multiple synthetic events."""
        return [self.generate_event() for _ in range(count)]

    def resolve(self, event: SyntheticEvent) -> bool:
        """Resolve an event based on its true probability.

        Returns True if the event happened (YES wins).
        """
        return self.rng.random() < event.true_probability

    def resolve_deterministic(self, event: SyntheticEvent, threshold: float = 0.5) -> bool:
        """Resolve deterministically based on probability threshold."""
        return event.true_probability >= threshold
