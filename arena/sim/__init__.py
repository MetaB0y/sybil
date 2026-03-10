"""Generic simulation framework for news-reactive LLM trading."""

from .clock import SimulatedClock
from .llm_trader import LlmTrader

__all__ = ["SimulatedClock", "LlmTrader"]
