---
tags: [arena]
layer: arena
status: current
last_verified: 2026-03-15
---

The LLM Trader uses a large language model as the decision engine for trading. Instead of hardcoded rules or quantitative models, it presents the LLM with market state, news, and portfolio context in a structured prompt, and the LLM returns analysis and trading orders. This is a fundamentally different approach from the other bots in the [[Bot Framework]]: the strategy is emergent from the LLM's reasoning rather than explicitly programmed.

The prompt structure gives the LLM everything it needs: the current market question, clearing prices, the bot's positions and balance, recent news headlines, and a persona (e.g., "you are an aggressive geopolitical analyst"). The LLM responds with an analysis of the situation and a set of order decisions. The trader parses the LLM output and converts it to [[Order Types|OrderSpecs]] for submission via the [[Python SDK]]. This makes the LLM Trader highly configurable — changing the persona or adding new information sources changes the trading behavior without modifying code.

The LLM Trader replaces the legacy `NewsTrader` which used a mechanical approach: Beta distribution belief updates from news headlines followed by Kelly criterion position sizing. The LLM approach is more flexible and can incorporate qualitative reasoning, but it's also less predictable and more expensive (each decision requires an API call). The simulation framework provides a `SimulatedClock` for time-compressed backtesting, so LLM traders can be evaluated against historical news data at accelerated speed.

## Key Properties
- LLM as decision engine — analysis + orders from a single prompt
- Structured prompt: market state, prices, positions, news, persona
- Configurable via persona and information sources, not code changes
- Replaces legacy NewsTrader (Beta belief + Kelly sizing)
- Each decision requires an LLM API call — slower and more expensive
- Backtestable via `SimulatedClock` time compression

## Where This Lives
> `arena/sim/llm_trader.py` — `LlmTrader` implementation
> `arena/sim/news_trader_legacy.py` — legacy `NewsTrader` for comparison
> `arena/markets/` — per-market personas, sources, and prompt configuration

## See Also
- [[Bot Framework]] — the base class `LlmTrader` extends
- [[Python SDK]] — order submission after LLM decision
- [[SSE Block Stream]] — market state delivered to inform LLM decisions
