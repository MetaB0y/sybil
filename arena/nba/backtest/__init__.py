"""Backtesting infrastructure for Sybil Arena."""

from .agent import BacktestAgent, BacktestAgentConfig, Belief
from .clock import SimulatedClock
from .dataset import Dataset, Event, FinalScore, MarketSpec, NewsItem
from .news import NewsScheduler, drain_queue
from .runner import AgentResult, BacktestResult, BacktestRunner, print_leaderboard

__all__ = [
    # Dataset
    "Dataset",
    "Event",
    "FinalScore",
    "MarketSpec",
    "NewsItem",
    # Clock
    "SimulatedClock",
    # News
    "NewsScheduler",
    "drain_queue",
    # Agent
    "BacktestAgent",
    "BacktestAgentConfig",
    "Belief",
    # Runner
    "AgentResult",
    "BacktestResult",
    "BacktestRunner",
    "print_leaderboard",
]
