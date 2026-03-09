"""Iran strike market configuration constants."""

from pathlib import Path

MARKET_QUESTION = "Will the United States carry out a military strike against Iran before March 31, 2026?"
MARKET_DESCRIPTION = "Resolves YES if US military strikes Iran before 2026-03-31"
MARKET_CATEGORY = "geopolitics"
INITIAL_PRICE = 0.12

CONTEXT = """\
Context:
USA-Iran tensions stem from long-standing issues like Iran's nuclear program and proxies, but escalated sharply after the June 2025 US strikes on Iranian nuclear sites during the Israel-Iran Twelve-Day War. They rose further in early January 2026 amid Iran's crackdown on anti-government protests, prompting President Trump to threaten military action and review strike options."""

ANALYSIS_QUESTION = "What does this article signal about the likelihood of a US strike on Iran by March 31?"

PHASE1_CRITERIA = """\
- US-Iran tensions, threats, or diplomatic signals
- Military moves, troop deployments, or defense posture involving US or Iran
- Iran nuclear program or sanctions
- Regional conflicts involving Iran (proxies, Israel, Gulf states)
- Iran domestic unrest that could trigger US intervention
- Market/economic reactions to US-Iran tensions (oil, safe havens, defense stocks)"""

PHASE1_PROMPT_TEMPLATE = """\
You are screening news headlines for a prediction market on US-Iran military conflict.

Market: "{market_question}"

Headline: "{{headline}}" -- {{source}}

Is this headline related to any of the following?
{phase1_criteria}

Say YES if the headline touches any of these topics, even indirectly.
Say NO only if the headline is clearly unrelated to US-Iran dynamics.
When in doubt, say YES -- it is cheap to filter later, expensive to miss relevant news.

Answer only YES or NO."""

# Directories
_MARKET_DIR = Path(__file__).parent
DATASETS_DIR = _MARKET_DIR / "datasets"
PHASE1_DIR = _MARKET_DIR / "tmp"
RUNS_DIR = _MARKET_DIR / "runs"
