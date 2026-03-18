"""Texas Republican Senate primary market configuration constants."""

from pathlib import Path

MARKET_QUESTION = "Will John Cornyn win the 2026 Texas Republican Senate primary?"
MARKET_DESCRIPTION = "Resolves YES if Cornyn wins the TX GOP Senate primary"
MARKET_CATEGORY = "politics"
INITIAL_PRICE = 0.61

CONTEXT = """\
Context:
The 2026 Texas Republican Senate primary pits incumbent Senator John Cornyn against \
Texas Attorney General Ken Paxton as the main contenders. Cornyn has substantial \
fundraising ($10M+) and party establishment backing, while Paxton appeals to the \
Trump-aligned populist base and won his 2024 AG primary decisively. A Trump endorsement \
would be the single biggest catalyst. The primary is in March 2026."""

ANALYSIS_QUESTION = "What does this article signal about Cornyn's chances of winning the Texas Republican Senate primary?"

PHASE1_CRITERIA = """\
- John Cornyn or Ken Paxton campaign news, strategy, or statements
- Texas Republican Senate primary polls, endorsements, or fundraising
- Trump endorsement signals or relationship with either candidate
- Texas GOP party dynamics, establishment vs populist tensions
- Other candidate entries, withdrawals, or endorsements
- Debate performance or campaign events in Texas"""

PHASE1_PROMPT_TEMPLATE = """\
You are screening news headlines for a prediction market on the Texas Republican Senate primary.

Market: "{market_question}"

Headline: "{{headline}}" -- {{source}}

Is this headline related to any of the following?
{phase1_criteria}

Say YES if the headline touches any of these topics, even indirectly.
Say NO only if the headline is clearly unrelated to the Texas Senate race or its key players.
When in doubt, say YES -- it is cheap to filter later, expensive to miss relevant news.

Answer only YES or NO."""

# Directories
_MARKET_DIR = Path(__file__).parent
DATASETS_DIR = _MARKET_DIR / "datasets"
PHASE1_DIR = _MARKET_DIR / "phase1"
RUNS_DIR = _MARKET_DIR / "runs"
POLYMARKET_PRICES_FILE = None
