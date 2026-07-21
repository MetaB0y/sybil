---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-18
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

The API, rather than each Arena consumer, owns external-price expiry. It
timestamps every market named by the mirror independently, publishes its exact
expiry, and omits it after `SYBIL_REFERENCE_PRICE_TTL_MS`; partial publisher
traffic cannot refresh an untouched token, and an API restart exposes no
reference until republish. Arena has no second CLOB/mapping cache: one shared
runtime view refreshes from the API every ten seconds, enforces the published
expiry locally, and clears immediately on a failed refresh. Missing or
non-positive values are unavailable, and `--require-reference-prices` does not
fall back to the startup snapshot or local clearing.

Each persona bus fans the same update out to two independent `LiveLlmTrader` subscribers. Despite
its retained historical class name, `LiveLlmTrader` is now LLM-free: it applies mechanical Kelly
or Flat sizing, decays stale fair values toward the observed market price, expires them after the
hard freshness limit, and emits [[Order Types|OrderSpecs]] through the [[Python SDK]]. Sharing the
exact analyst update keeps the sizing comparison attributable to sizing rather than duplicate
LLM calls or divergent news inputs.

LLM cost and failure are contained at the analyst boundary. Each analyst has a persistent spend
pause threshold; reaching it blocks the next call without affecting other personas. Because cost
is known only after a completed call, actual spend may exceed the threshold by that final call.
Parse fallbacks are counted. Provider capability is tracked independently from
that local experiment budget: relevance-gate and analyst calls classify
authentication, credit, rate-limit, timeout, upstream, and other failures;
publish degraded/last-success/backoff metrics; and use bounded backoff for
401/402/429. Failed analyst calls retain their evidence batch for retry and
still consume the normal call cadence, so a transient failure cannot become a
per-block retry loop. Relevance-gate failure passes already-seen candidates to
the analyst rather than destroying evidence. The relevance classifier disables
its model's optional reasoning explicitly: its bounded completion budget belongs
to the exact `NONE` or comma-separated-index answer, not hidden reasoning tokens.
A successful provider response clears the degraded state. Alerts, Grafana, and
`live.status` make this capability distinct from container health and local budget
remaining.

A durable sizer decision is written exactly once,
when a fresh `FairValueUpdate` is first applied; it records the raw/effective fair value, age,
confidence, restatement, countercase, rejection reason, article sources, and proposed orders.
Timer-only position management may still emit replacement orders, but it does not duplicate the
forecast row. Proposed orders are not evidence of API acceptance; accepted-order and fill totals
come from portfolio snapshots. Offline
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
the current per-market article list and one still-unexpired API reference price. Each arm receives
that exact evidence-and-price batch once, and the next pending batch remains blocked until both
views acknowledge the active batch. A failed provider attempt releases only
that arm's lease so it can retry the identical batch; the other arm cannot
advance the barrier. When the reference is unavailable, articles remain queued rather
than being paired with the frozen experiment-start value.
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

An active experiment also starts an Arena-owned authoritative outcome recorder for exactly the
frozen `market_ids` cohort. It checks immediately and then every 900 seconds by default; operators
can set a positive `--outcome-record-interval-s` or `ARENA_OUTCOME_RECORD_INTERVAL_S` value. The
CLI cadence flag is rejected without an active experiment; a dormant environment value is ignored.
The ordinary topology starts no recorder task. Resolution labels come from Sybil's
`/v1/markets/{id}/resolution` endpoint and are inserted immutably into `market_outcomes` through a
WAL writer. Before a sweep and again immediately before its write transaction, the recorder
requires `/v1/health` to match the nonzero 32-byte genesis hash persisted in the experiment
metadata. A reset between those checks cannot land old-chain labels. Exact-cohort 404s are fatal
drift; the manual decisions-derived compatibility mode may still treat 404 as missing. Cooperative
stop checks before and after health calls, between market fetches, and before writes bound shutdown
and prevent later cohort requests or commits after stop. Network/HTTP and SQLite operational
failures warn and retry at the next interval. A chain mismatch, invalid response, changed outcome,
or unexpected recorder defect emits a critical log and
permanently disarms only the recorder while it remains alive until shutdown; analyst and trading
tasks continue. The standalone `scripts.record_outcomes` CLI retains decisions-derived scope by
default and accepts `--market-ids` for an exact manual cohort.

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

After at least a day, generate the strict report for both durable variants over the persisted
half-open window and frozen cohort:

```bash
cd arena
uv run python -m scripts.calibration \
  --db live/decisions.db \
  --experiment-id stage1-2026-07-12-a \
  --until <exclusive-utc-window-end> \
  --json-out stage1-ab-calibration.json
```

The strict report derives its inclusive start and cohort from `live_experiments`; supplying
`--since` or `--market-ids` alongside the experiment id is rejected. It fingerprint-checks and
allowlists the exact experiment analyst/Flat identities, then reports per-arm calls, USD spend,
cost per analysis batch, matched-batch coverage, Stage1-minus-control spend deltas,
and Flat PnL. Windows shorter than 24 hours are refused unless
`--exploratory-short-window` is explicitly supplied; that override is recorded in text and JSON
and is not launch evidence. A non-exploratory report also proves that every exact Flat arm has
durable portfolio snapshots across the requested window: its first row must be within ten minutes
of the persisted start, its latest within ten minutes of the exclusive end, and no consecutive gap
may exceed ten minutes against the normal 300-second cadence. Per-arm first/latest timestamps,
maximum gap, and status are emitted in text and JSON; incomplete coverage is rejected. Exploratory
reports retain but loudly label incomplete coverage. PnL and spend remain available before
resolution. The exclusive end must not be in the future; endpoint tolerance can absorb snapshot
cadence jitter but can never turn a future interval into evidence. Brier, reliability, and
rejection counterfactuals use explicit outcomes only in strict experiment mode. Outcome rows are
filtered to the frozen cohort before their source/count is reported, so unrelated labels cannot
hide that the experiment itself remains unresolved. Starting a different cohort requires a new
experiment id.

Each freshly applied fair-value update also persists a deterministic
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
- Provider credit/auth health is explicit, independently metered, and evidence-preserving
- One decision row per fresh forecast application, plus outcome records, supports
  windowed, cohort-pinned calibration
- Opt-in Stage 1 A/B uses paired evidence batches, isolated Flat arms, and single-run ids
- Persisted Stage 1 reports lock scope to immutable metadata and fail closed on identity drift
- Strict Stage 1 evidence requires uninterrupted durable snapshot coverage for every Flat arm
- Simulation remains backtestable through `SimulatedClock` time compression

## Where This Lives
> `arena/sim/llm_trader.py` — portfolio-aware simulation `LlmTrader`
> `arena/live/analyst.py` — live portfolio-agnostic `PersonaAnalyst`
> `arena/live/fair_value_bus.py` — per-persona analyst-to-sizer fan-out
> `arena/live/trader.py` — mechanical live sizer (`LiveLlmTrader`)
> `arena/live/strategy.py` — Kelly/Flat sizing and fair-value freshness
> `arena/live/db.py` — immutable `live_experiments` restart metadata
> `arena/live/outcomes.py` — authoritative immutable outcome recording and experiment loop
> `arena/live/runner.py` — default and opt-in concurrent Stage 1 topologies
> `arena/scripts/calibration.py` — offline forecast and PnL comparison
> `arena/scripts/calibration_compare.py` — exact-cohort, exact-account window deltas
> `arena/markets/` — per-market simulation personas, sources, and prompts

## See Also
- [[Bot Framework]] — account-holding agents and the live sizer boundary
- [[Python SDK]] — order submission after mechanical sizing
- [[WebSocket Block Stream]] — first-party resumable market-state stream
