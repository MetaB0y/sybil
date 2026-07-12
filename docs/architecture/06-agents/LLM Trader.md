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
pause threshold; reaching it blocks the next call without affecting other personas. Because cost
is known only after a completed call, actual spend may exceed the threshold by that final call.
Parse fallbacks are
counted, while every sizer decision records the raw/effective fair value, age, confidence,
restatement, countercase, rejection reason, article sources, and resulting orders. Offline
calibration uses explicit resolution outcomes when available and can pin both time windows and
market cohorts. This measurement layer is required before adding higher-spend techniques such as
price-move re-estimation or a second-opinion model.

### Concurrent Stage 1 experiment

The default live topology remains one current `PersonaAnalyst` feeding Kelly and Flat sizers. An
operator can opt into the SYB-114 concurrent Stage 1 comparison by supplying both
`--stage1-ab-experiment-id <id>` and an explicit nonempty `--market-ids <id...>` cohort. In this
mode, each configured persona instead gets two isolated arms on the same `NewsFeed` and exact
market set:

- `control`: the pre-Stage-1 analyst prompt/output contract (no required `RESTATE` and no
  Stage-1 source-discipline rule), its own `FairValueBus`, and one Flat sizer/account;
- `stage1`: the current RESTATE + source-discipline contract, its own `FairValueBus`, and one
  Flat sizer/account.

Analyst, sizer, bus, and account identities include the experiment id, persona, and variant, so
token spend, decisions, snapshots, and portfolios cannot be confused across arms. Each persona's
two analysts share one feed subscription through a paired batch barrier. The first drain snapshots
the current per-market article list and one reference price (a positive fresh Polymarket value when
available, otherwise the frozen startup reference). Each arm receives that exact evidence-and-price
batch once, and the next pending batch remains blocked until both views consume the active batch.
A paused or lagging arm therefore cannot let the other advance onto differently grouped evidence or
a later price context. Experiment startup rejects any selected market without a positive external
reference before metadata or accounts are created. The per-analyst LLM pause threshold is unchanged.
Two analysts per persona make the configured per-persona threshold exactly twice the ordinary
one-analyst threshold (for example, `$5 + $5 = $10`); this is not a hard ceiling because each arm may
cross its threshold by one completed call.

Activation is opt-in and does not alter the Compose default. Use a new experiment id for each
measurement and chain genesis. Operators can either pass both CLI flags:

```bash
cd arena
OPENROUTER_API_KEY="$OPENROUTER_API_KEY" uv run python -m live.runner \
  --stage1-ab-experiment-id stage1-2026-07-12-a \
  --market-ids 17 29 44 \
  --personas news_trader contrarian fundamentals \
  --llm-budget-usd 5 \
  --db-path live/decisions.db
```

or set the paired environment fallbacks in `/opt/sybil/arena.env`, which Compose already supplies
to the arena container:

```bash
ARENA_STAGE1_AB_EXPERIMENT_ID=stage1-2026-07-12-a
ARENA_MARKET_IDS=17,29,44
```

CLI values override their corresponding environment values. Empty or unset variables preserve the
ordinary topology exactly. Environment activation fails closed unless both variables are present;
`ARENA_MARKET_IDS` alone never changes ordinary automatic market selection. The CLI continues to
allow `--market-ids` by itself for non-experiment manual selection.

Before resolving any experiment account, startup requires `/v1/health` to report height at least 1
and a nonzero 32-byte `genesis_hash`. On first start, `live_experiments` in `decisions.db` records
that chain identity together with the experiment mode, UTC start, exact normalized market-id cohort,
model, exact LLM generation parameters, SHA-256 fingerprints of each complete static prompt
contract plus selected persona text and display name, analyst/sizer counts, LLM pause thresholds,
trading budgets, cadence, freshness settings, and order time-in-force. Experiment ids are
single-run: even an exactly compatible restart is rejected because analyst fair values and Flat
entry-price state cannot be reconstructed safely. The retained record diagnoses configuration
drift, but every process restart, partial startup, or fresh genesis requires a new experiment id.

Before any feed, analyst, or trader task starts, the runner also awaits a one-shot portfolio
snapshot for every live, fast, and noise account. An experiment-arm snapshot failure aborts startup
and invalidates that id; default and synthetic snapshot failures remain warning-only and retry on
the periodic loop. This time-zero row ensures first-interval PnL is included in the half-open window.

After at least a day (and once explicit outcomes exist), compare both durable variant names over the
persisted half-open window and frozen cohort:

```bash
cd arena
uv run python scripts/calibration.py \
  --db live/decisions.db \
  --since <started_at_utc-from-live_experiments> \
  --until <exclusive-utc-window-end> \
  --market-ids <comma-separated-frozen-ids> \
  --json-out stage1-ab-calibration.json
```

The calibration report keeps `[...]control]` and `[...]stage1]` trader identities distinct for
Brier, reliability, rejection, and per-trader Flat-arm PnL comparison. `token_usage` uses the same
durable variant identity for a direct spend audit. Starting a different cohort requires a new
experiment id. Per-trader PnL can be read during an unresolved experiment; Brier, reliability, and
rejection counterfactuals require explicit resolved outcomes and should not be treated as final
before those labels exist. Each fair-value update and decision also persists a deterministic
`analysis_batch_id` derived from the market id, snapped reference price, and sorted article URLs.
The same snapped price is persisted as `analysis_reference_price` and supplies both arms' prompt,
resolution check, and matched market-price baseline even if the provider moves between drains.
Forecast scoring keeps only the first row per durable trader, market, and batch. The primary Stage 1
comparison computes control/Stage-1 Brier and analysis-price baseline only on the exact batch-id
intersection, with Stage1-minus-control deltas; zero matches are explicitly not comparable.
Full-arm metrics remain diagnostic and report asymmetric batches. Cross-window PnL comparison uses
only the exact intersection of durable trader names per arm and reports every excluded identity.

For a before/after comparison, use explicit half-open windows. The command fails if the Flat arm
has no exact durable-name intersection, which catches account/cohort changes instead of subtracting
unrelated aggregate means:

```bash
cd arena
uv run python -m scripts.calibration_compare \
  --before-db before.db \
  --before-since <inclusive-utc-start> \
  --before-until <exclusive-utc-end> \
  --after-db after.db \
  --after-since <inclusive-utc-start> \
  --after-until <exclusive-utc-end>
```

## Key Properties
- Simulation: one portfolio-aware LLM produces analysis and orders
- Live arena: one portfolio-agnostic analyst publishes fair values per persona
- Per-persona `FairValueBus` gives Kelly and Flat sizers identical analyst inputs
- Live order sizing and freshness handling are deterministic and LLM-free
- Persistent per-analyst spend thresholds pause only that analyst's later calls
- Decision and outcome records support windowed, cohort-pinned calibration
- Opt-in Stage 1 A/B uses paired evidence batches, isolated Flat arms, and single-run ids
- Simulation remains backtestable through `SimulatedClock` time compression

## Where This Lives
> `arena/sim/llm_trader.py` — portfolio-aware simulation `LlmTrader`
> `arena/live/analyst.py` — live portfolio-agnostic `PersonaAnalyst`
> `arena/live/fair_value_bus.py` — per-persona analyst-to-sizer fan-out
> `arena/live/trader.py` — mechanical live sizer (`LiveLlmTrader`)
> `arena/live/strategy.py` — Kelly/Flat sizing and fair-value freshness
> `arena/live/db.py` — immutable `live_experiments` restart metadata
> `arena/live/runner.py` — default and opt-in concurrent Stage 1 topologies
> `arena/scripts/calibration.py` — offline forecast and PnL comparison
> `arena/scripts/calibration_compare.py` — exact-cohort, exact-account window deltas
> `arena/markets/` — per-market simulation personas, sources, and prompts

## See Also
- [[Bot Framework]] — account-holding agents and the live sizer boundary
- [[Python SDK]] — order submission after mechanical sizing
- [[WebSocket Block Stream]] — first-party resumable market-state stream
