"""Trading bot implementations."""

from .base import BaseAgent
from .flash_mm import FlashMarketMaker, TightFlashMM, WideFlashMM
from .informed import FixedProbabilityModel, InformedTrader, MomentumTrader, ProbabilityModel
from .market_maker import SimpleMarketMaker
from .random_trader import RandomTrader

__all__ = [
    "BaseAgent",
    "FixedProbabilityModel",
    "FlashMarketMaker",
    "InformedTrader",
    "MomentumTrader",
    "ProbabilityModel",
    "RandomTrader",
    "SimpleMarketMaker",
    "TightFlashMM",
    "WideFlashMM",
]


def __getattr__(name):
    """Lazy import for backtest-dependent bots to avoid circular imports."""
    if name in ("NewsTrader", "ConservativeNewsTrader", "AggressiveNewsTrader"):
        from .news_trader import AggressiveNewsTrader, ConservativeNewsTrader, NewsTrader
        return {"NewsTrader": NewsTrader, "ConservativeNewsTrader": ConservativeNewsTrader, "AggressiveNewsTrader": AggressiveNewsTrader}[name]
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
