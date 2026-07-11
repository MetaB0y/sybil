---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-12
---

Sybil has two LLM trading paths with deliberately different boundaries.

The simulation path in `arena/sim/llm_trader.py` lets one `LlmTrader` see market state,
portfolio state, news, and a persona, then return analysis and orders. It is useful for
time-compressed backtests with `SimulatedClock`, but it is not the live arena architecture.

The live path separates forecasting from money management. One portfolio-agnostic
`PersonaAnalyst` per persona drains that persona's news subscription, calls the analysis LLM,
and publishes a structured `FairValueUpdate` to a `FairValueBus`. The prompt includes the exact
market question and resolution criteria, the current external reference price, the prior fair
value, and clustered articles. Its response must restate the YES condition before giving a fair
value, countercase, confidence, motivation, and analysis. The analyst has no Sybil account and
cannot submit orders.

Each persona bus fans the same update out to two independent `LiveLlmTrader` subscribers. Despite
its retained historical class name, `LiveLlmTrader` is now LLM-free: it applies mechanical Kelly
or Flat sizing, decays stale fair values toward the observed market price, expires them after the
hard freshness limit, and emits [[Order Types|OrderSpecs]] through the [[Python SDK]]. Sharing the
exact analyst update keeps the sizing comparison attributable to sizing rather than duplicate
LLM calls or divergent news inputs.

LLM cost and failure are contained at the analyst boundary. Each analyst has a persistent spend
budget; exhaustion pauses new analysis without affecting other personas. Parse fallbacks are
counted, while every sizer decision records the raw/effective fair value, age, confidence,
restatement, countercase, rejection reason, article sources, and resulting orders. Offline
calibration uses explicit resolution outcomes when available and can pin both time windows and
market cohorts. This measurement layer is required before adding higher-spend techniques such as
price-move re-estimation or a second-opinion model.

## Key Properties
- Simulation: one portfolio-aware LLM produces analysis and orders
- Live arena: one portfolio-agnostic analyst publishes fair values per persona
- Per-persona `FairValueBus` gives Kelly and Flat sizers identical analyst inputs
- Live order sizing and freshness handling are deterministic and LLM-free
- Persistent per-analyst spend caps fail by pausing only that analyst
- Decision and outcome records support windowed, cohort-pinned calibration
- Simulation remains backtestable through `SimulatedClock` time compression

## Where This Lives
> `arena/sim/llm_trader.py` — portfolio-aware simulation `LlmTrader`
> `arena/live/analyst.py` — live portfolio-agnostic `PersonaAnalyst`
> `arena/live/fair_value_bus.py` — per-persona analyst-to-sizer fan-out
> `arena/live/trader.py` — mechanical live sizer (`LiveLlmTrader`)
> `arena/live/strategy.py` — Kelly/Flat sizing and fair-value freshness
> `arena/scripts/calibration.py` — offline forecast and PnL comparison
> `arena/markets/` — per-market simulation personas, sources, and prompts

## See Also
- [[Bot Framework]] — account-holding agents and the live sizer boundary
- [[Python SDK]] — order submission after mechanical sizing
- [[WebSocket Block Stream]] — first-party resumable market-state stream
