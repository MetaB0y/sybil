"""Per-market configuration for simulations."""

from dataclasses import dataclass
from pathlib import Path
from typing import Callable


@dataclass
class MarketConfig:
    """Configuration for a specific prediction market simulation."""
    question: str
    description: str
    category: str
    initial_price: float
    context: str
    analysis_question: str
    phase1_criteria: str
    phase1_prompt_template: str
    datasets_dir: Path
    phase1_dir: Path
    runs_dir: Path
    personas: dict
    build_persona: Callable[[dict], str]
    polymarket_prices_file: Path | None = None
