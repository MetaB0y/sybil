---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-11
---

The LLM Trader uses a large language model as the decision engine for trading. Instead of hardcoded rules or quantitative models, it presents the LLM with market state, news, and portfolio context in a structured prompt, and the LLM returns analysis and trading orders. This is a fundamentally different approach from the other bots in the [[Bot Framework]]: the strategy is emergent from the LLM's reasoning rather than explicitly programmed.

The prompt structure gives the LLM everything it needs: the current market question, clearing prices, the bot's positions and balance, recent news headlines, and a persona (e.g., "you are an aggressive geopolitical analyst"). The LLM responds with an analysis of the situation and a set of order decisions. The trader parses the LLM output and converts it to [[Order Types|OrderSpecs]] for submission via the [[Python SDK]]. This makes the LLM Trader highly configurable — changing the persona or adding new information sources changes the trading behavior without modifying code.

The LLM path is flexible but less predictable and more expensive than a mechanical strategy because each decision may require a model call. The simulation framework provides a `SimulatedClock` for time-compressed backtesting against historical news.

## Key Properties
- LLM as decision engine — analysis + orders from a single prompt
- Structured prompt: market state, prices, positions, news, persona
- Configurable via persona and information sources, not code changes
- Each decision requires an LLM API call — slower and more expensive
- Backtestable via `SimulatedClock` time compression

## Where This Lives
> `arena/sim/llm_trader.py` — `LlmTrader` implementation
> `arena/markets/` — per-market personas, sources, and prompt configuration

## See Also
- [[Bot Framework]] — the base class `LlmTrader` extends
- [[Python SDK]] — order submission after LLM decision
- [[WebSocket Block Stream]] — first-party resumable market-state stream
