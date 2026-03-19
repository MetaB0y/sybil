"""China visit market configuration constants."""

from pathlib import Path

MARKET_QUESTION = "Will Trump visit China by March 31?"
MARKET_DESCRIPTION = "Resolves YES if Trump travels to mainland China before 2026-03-31"
MARKET_CATEGORY = "geopolitics"
INITIAL_PRICE = 0.015

CONTEXT = """\
Context:
US-China relations remain strained over trade tariffs, Taiwan, and technology restrictions. \
Trump has oscillated between confrontation and deal-making with Xi Jinping. \
A presidential visit to China would signal a major diplomatic thaw, but requires \
weeks of advance planning and bilateral groundwork. No visit has been announced or \
publicly scheduled as of mid-February 2026."""

ANALYSIS_QUESTION = "What does this article signal about the likelihood of Trump visiting China by March 31?"

PHASE1_CRITERIA = """\
- Trump-Xi Jinping interactions: meetings, calls, diplomatic exchanges, personal rhetoric
- US-China trade: tariffs, negotiations, deals, sanctions, export controls, trade war
- Presidential travel or state visits involving Trump and China/Asia
- US-China diplomatic signals: breakthroughs, escalations, back-channel talks
- Taiwan: tensions, de-escalation, military posturing, arms sales
- High-level US-China official contacts (any cabinet-level or above)
- China policy from the White House, State Dept, Congress, or Pentagon
- US-China economic/financial dynamics: yuan, investment, supply chains, decoupling
- Geopolitical context: US alliances in Asia, AUKUS, Quad, ASEAN positioning vis-a-vis China"""

PHASE1_PROMPT_TEMPLATE = """\
You are screening news headlines for a prediction market about US-China relations.

Market: "{market_question}"

Headline: "{{headline}}" -- {{source}}

Is this headline related to US-China relations, diplomacy, trade, or geopolitics?
Relevant topics include:
{phase1_criteria}

Say YES if the headline touches US-China dynamics in ANY way, even indirectly.
Say NO only if the headline is clearly unrelated to US-China relations.
When in doubt, say YES -- we filter further in later stages.

Answer only YES or NO."""

# Directories
_MARKET_DIR = Path(__file__).parent
DATASETS_DIR = _MARKET_DIR / "datasets"
PHASE1_DIR = _MARKET_DIR / "phase1"
RUNS_DIR = _MARKET_DIR / "runs"
POLYMARKET_PRICES_FILE = _MARKET_DIR / "polymarket_prices.json"
