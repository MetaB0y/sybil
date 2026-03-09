"""Generic simulation framework for news-reactive LLM trading."""

from .clock import SimulatedClock
from .news_trader import NewsTrader

__all__ = ["SimulatedClock", "NewsTrader"]
