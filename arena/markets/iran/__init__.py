"""Iran strike market configuration."""

from pathlib import Path

from markets import MarketConfig

from .config import (
    ANALYSIS_QUESTION,
    CONTEXT,
    DATASETS_DIR,
    INITIAL_PRICE,
    MARKET_CATEGORY,
    MARKET_DESCRIPTION,
    MARKET_QUESTION,
    PHASE1_CRITERIA,
    PHASE1_DIR,
    PHASE1_PROMPT_TEMPLATE,
    RUNS_DIR,
)
from .personas import BOT_PERSONAS


def build_persona(bot_config: dict) -> str:
    """Build a full persona prompt from a BOT_PERSONAS entry."""
    p = bot_config["persona"]
    style_lines = "\n".join(f"- {s}" for s in p["style"])
    return f"""\
You are {p['identity']}.

You're trading on the market: "{MARKET_QUESTION}"

{CONTEXT}

Your analytical style:
{style_lines}"""


def get_config() -> MarketConfig:
    """Return the MarketConfig for the Iran strike market."""
    return MarketConfig(
        question=MARKET_QUESTION,
        description=MARKET_DESCRIPTION,
        category=MARKET_CATEGORY,
        initial_price=INITIAL_PRICE,
        context=CONTEXT,
        analysis_question=ANALYSIS_QUESTION,
        phase1_criteria=PHASE1_CRITERIA,
        phase1_prompt_template=PHASE1_PROMPT_TEMPLATE.format(
            market_question=MARKET_QUESTION,
            phase1_criteria=PHASE1_CRITERIA,
        ),
        datasets_dir=DATASETS_DIR,
        phase1_dir=PHASE1_DIR,
        runs_dir=RUNS_DIR,
        personas=BOT_PERSONAS,
        build_persona=build_persona,
    )
