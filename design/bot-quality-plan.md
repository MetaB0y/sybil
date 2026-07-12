---
tags: [arena, bots, calibration, research, terminator2]
status: proposal-needs-revalidation
date: 2026-07-10
ticket: SYB-114
---

# SYB-114 research: terminator2-agent techniques for the live arena bots

> **Status note (2026-07-11):** research input, not the current arena backlog.
> Resurvey the live bot pipeline and calibration data before executing a stage.
>
> **Stage 1 harness update (2026-07-12):** the runner now has an opt-in concurrent
> `--stage1-ab-experiment-id` mode. It freezes an explicit `--market-ids` cohort and runs isolated
> pre-Stage-1 control/current-Stage-1 Flat arms on the same feed. The ordinary live topology is
> unchanged. Metadata is also bound to the committed chain `genesis_hash`, so a fresh genesis needs
> a new experiment id. A paired batch barrier holds each article batch until both arms consume it,
> snapshots one shared reference price for both prompts, and experiment ids are single-run because
> in-memory analyst/Flat state cannot resume safely.
> This documents available measurement machinery, not a completed window.

Stage 0 measurement and the opt-in Stage 1 harness are implemented, but no qualifying experiment
window has completed. Further spend-increasing stages remain gated on a strict 24-hour Stage 1
report after the frozen cohort has authoritative resolutions. This note is the research half plus
the staged plan and implementation status.

## 1. Our baseline (what the live bots actually do today)

All paths under `/home/anonymous/sybil/arena/`.

**Architecture (post SYB-210 analysis/sizing split):**
- `live/analyst.py` — one `PersonaAnalyst` per persona (3 personas: news_trader,
  contrarian, fundamentals — `live/personas.py`). It is the persona's **sole LLM caller**.
  News-triggered only: it drains article batches from the shared `NewsFeed`, makes **one
  single-shot LLM call per (market, article-batch)** (`_call_llm`, temperature 0.3,
  max_tokens 2048, reasoning max 1024), parses `FAIR_VALUE / COUNTERCASE / CONFIDENCE /
  MOTIVATION / ANALYSIS` with regex, and publishes a `FairValueUpdate` on a per-persona
  `FairValueBus`. No ensembling, no self-consistency, no second opinion, no retry-on-parse-fail
  (fallback defaults: confidence 0.5).
- Model: **OpenRouter `deepseek/deepseek-v4-flash`** (default in `live/runner.py::LiveConfig`),
  spend pause threshold **$5/analyst default** (`llm_budget_usd`); actual billed cost arrives via
  `usage.include` (`live/costs.py`) after a call, so spend may cross the threshold by that final
  call before later calls pause. Min 60s between LLM calls (`min_llm_interval_s`).
- `live/news_feed.py` — Google News RSS per market (query built from market name), URL dedup,
  batch **LLM relevance gate** (`google/gemma-4-31b-it`, 1 call/market/poll), full text via
  trafilatura, 300s poll. Also fetches **Polymarket CLOB mid prices** (the cross-platform
  reference that goes into the analyst prompt). Near-duplicate article clustering by token
  Jaccard (`analyst.py::cluster_near_duplicate_articles`, threshold 0.82).
- `live/trader.py` — `LiveLlmTrader` is a **mechanical, LLM-free sizer**; two arms per persona
  (Kelly ⅓ + Flat $20, `live/strategy.py`) consume the *same* bus so A/B inputs are provably
  identical (SYB-192/210). FV freshness decay: fresh ≤10min, edge half-life 30min, hard expiry
  2h (`strategy.py::effective_fair_value`). Kelly shrinks size by `confidence × freshness`;
  **Flat ignores confidence entirely** (`FlatStrategy.target` `del confidence`).
  Note: FlatStrategy is already explicitly "terminator2-inspired" (strategy.py:198-227).
- `bots/base.py` — generic block loop (10s blocks): stream blocks, skip when pending orders,
  submit orders, per-block error isolation. `bots/informed.py`/`market_maker.py` are sim bots,
  not the live loop.

**What gets logged per decision** (`live/db.py::decisions` table): trader_name, market_id,
fair_value, raw/effective FV, fair_value_age_s, confidence, countercase, market_price, orders
(JSON), motivation, analysis, raw_llm_response, balance, positions, article_urls, an explicit
`rejection_reason` for every no-order sizing decision, and market category/tags. Token spend per
call is stored in `token_usage`. Portfolio snapshots are written once before workers start and
every 300s thereafter.

**Calibration measurement today** (`scripts/calibration.py`): per-persona Brier vs
market-price-as-forecast baseline, reliability curve, rejection calibration
(rejected = decision row with no orders), NativeNoiseTrader PnL baseline. Stage 1 experiments now
poll authoritative Sybil resolutions for their exact persisted cohort and write immutable
`market_outcomes` rows automatically; the manual recorder remains available for other runs. The
report groups rejection counterfactuals by persisted reason, emits category-level calibration and
the largest forecast surprises, and supports pinned half-open windows and cohorts.

Stage 0 measurement plumbing is complete. A strict Stage 1 result still depends on running the
concurrent experiment for at least 24 hours and waiting for authoritative cohort resolutions; that
is evidence collection, not missing measurement implementation.

## 2. terminator2-agent technique map

Source repo: `github.com/terminator2-agent/terminator2-agent.github.io` (GitHub Pages site;
pages render from JSON exported by the agent). Agent: Claude Opus 4.6, ~20-min heartbeat,
trades **Manifold Markets** since 2026-02, built by "marbinner". Headline stats
(`portfolio_stats.json`, updated 2026-07-10): ROI 15.7%, total PnL M$142.5k, win rate 0.67,
**Brier 0.0271 over 8,355 resolved samples** (heavily weighted to easy 0-10%/90-100% buckets).

### Documented techniques (with source file/page)

| # | Technique | What it is | Source |
|---|-----------|------------|--------|
| T1 | **Pre-cycle detection pipeline** | Deterministic tools run before the LLM wakes: price cache (refresh 15min, "catches moves >10pp"), edge signals ("cross-platform reference, flags disagreements >10pp" — Metaculus/Vegas de-vig), catalog scanner (Vertex/Bedrock model catalogs as leading indicators), briefing digest. Motto: "The tools detect; I decide." | `about.html` "How I work" |
| T2 | **Oracle second-opinion** | A fast, *different* LLM (Gemini) independently estimates each market; gaps vs own estimate become "challenge" decisions (e.g. "Oracle: 55% vs my 40% (15pp gap)"). Explicitly "catches overconfidence." | `about.html`; `decisions.json` category `challenge` (466 of 5,762 proposals) |
| T3 | **Oracle hallucination tracking** | Machine-readable log of oracle factual failures with an error typology: existence_denial, present_tense_fabrication, stale_world_model, extreme_miscalibration, **question_comprehension_failure** ("oracle reads the title but misses resolution criteria"). Meta-rule: "the bigger the oracle's confidence on a verifiable claim, the more verification is needed." | `oracle_accuracy.json` |
| T4 | **Self-rules from mistakes** | 86 rules extracted from losses over 1,560 cycles, enforced as a pre-trade checklist every cycle. Machine-readable mistake ledger with `amount_lost`, `category`, `self_rules_spawned`, `fix_status`. Examples: "when my estimate crosses 70% against my position, sell at least 50% immediately" (M$429 lesson); "paraphrase the question before you enter" (resolution criteria, not vibes); "verify government data against primary source, not web search summaries"; "3+ independent leaks converge → 'when' not 'if'"; "never bet more than M$25 on a naming thesis". | `mistakes.json`; `about.html` "Costly lessons" (10 lessons, ~M$1,930 tuition) |
| T5 | **Rejection calibration with reasons + counterfactuals** | Every rejected trade logged with reason (stale_estimate_hard_reject, thin_edge_at_fill, concentration_cap, cluster_cap_*, balance_floor…) and later scored: "would_have_profited". Overall rejection accuracy 0.6; per-reason data shows which rules earn (stale-estimate reject: only 39/416 would have profited = good rule) and which cost (concentration cap: 1,329/1,520 would have profited = expensive insurance). Performance page: "How well do I refuse? Counterfactual analysis of trades I didn't make." | `portfolio_stats.json::rejection_calibration`; `performance.html` |
| T6 | **Fractional Kelly limit orders + hard caps** | Third-Kelly limit orders proposed by pipeline, agent approves/rejects each. Caps: M$300/market (after M$960 correlated-answers loss), ≥M$40 free capital, tranche exits in thin markets. Correlated-cluster exposure tracked (`correlated_risk` clusters with worst-case loss). | `about.html`; `portfolio_stats.json::correlated_risk`; `decisions.json` category `limit` |
| T7 | **Calibration self-tracking** | Public Brier + reliability buckets + by-category Brier + "surprises" list (biggest forecast-outcome gaps) + overconfidence ratio. | `portfolio_stats.json::calibration_metrics` |
| T8 | **Primary-source verification** | "The web is full of algorithmically generated press releases describing products that don't exist"; search subagents were fooled repeatedly → rule: verify releases against the company's own blog. Catalog scanners as *primary* leading indicator. | `about.html` "Costly lessons"; `mistakes.json` (M$70 fabricated-number loss) |
| T9 | **Devil's-advocate stress tester** | "Devil's advocate via LLM. Finds blind spots in theses and clusters." | `about.html` "How I work" |
| T10 | **Public decision log** | `decisions.json`: proposed vs executed per cycle with reasoning, oracle_estimate, edge. | repo file listing; README.md |

### Inferred / notable absences
- **No statistical calibration post-processing found** — no extremization, Platt/temperature
  scaling, or base-rate anchoring layer anywhere in the published material. Their calibration
  edge comes from *behavioral* machinery (oracle challenge, self-rules, rejection discipline),
  not from transforming probabilities. (Inferred from absence; agent code itself is not public —
  only the site exporter scripts are.)
- Per GitHub issue #22 (`changelog.json`): **~$4–6/cycle in Opus tokens, $1–2/cycle for pure
  trading** — i.e. one terminator2 cycle costs ~⅓ of one of our analysts' *lifetime* budgets.
  The raw stack does not transplant; the cheap deterministic scaffolding does.
- Their profile is NO-heavy grinding of longshots (3,495 forecasts in the 0-10% bucket vs 10 in
  20-30%): much of the Brier 0.027 is easy mass, not magic.

## 3. Transfer analysis

Setting constraints: 10s blocks, EG/Fisher batch clearing, IOC orders, $5–10 spend-threshold
OpenRouter keys, deepseek-v4-flash analysts, continuous operation, Kelly sizing loop OUT of
scope (known FV-drift concern).

| T# | Transfers? | Our version | Expected effect | Cost | Spend impact |
|----|-----------|-------------|-----------------|------|--------------|
| T5 rejection reasons + counterfactuals | **Implemented** | Every no-order sizer decision persists a reason; `calibration.py` reports per-reason would-have-profited counterfactuals | Measurement; indirectly PnL via tuning min_edge | S | zero |
| T1 price-move trigger | **Yes — highest PnL** | Analyst currently re-estimates *only on news*. Add: if Polymarket ref moved >8-10pp since last FV (cache already in `news_feed.PolymarketPrices`), enqueue a re-estimate even without articles. Directly attacks stale-FV / FV-drift | PnL + variance (fewer stale-FV losses); modest calibration | M | +20-50% analyst calls, bounded by existing 60s interval + budget pause; gate threshold conservatively |
| T2 oracle second-opinion | **Yes** | Second cheap model (reuse gate-class `google/gemma-4-31b-it` or another flash model, different family than deepseek) gives independent FV; if gap >15pp → clamp confidence (e.g. ×0.5) or hold prior. Log both estimates | Calibration (overconfidence catch), variance ↓ | M | +1 cheap call per analysis ≈ +10-20% cost; or edge-triggered only (call oracle only when analyst wants to move FV >10pp) ≈ +5% |
| T4 self-rules ledger | **Yes (semi-manual)** | `mistakes` table fed from calibration "surprises" (biggest forecast-outcome gaps with orders); per-persona rules appended to persona prompt after each calibration window. Start manual (Valery/agent writes 3-5 rules from window 1), automate later | Calibration + PnL, slow compounding | M | negligible (+~100 prompt tokens) |
| T3 comprehension guard | **Implemented** | Required `RESTATE:` paraphrase is parsed and logged before fair value; the A/B control retains the old contract | Calibration on ambiguous markets | S | +~40 output tokens/call (~2-4%) |
| T8 source discipline | **Implemented (lite)** | Prompts identify article sources and explicitly discount aggregator/SEO summaries; full domain-tier classification remains optional | Calibration (fabricated/SEO news discount) | S | zero |
| T7 surprises list | **Implemented** | `calibration.py` emits largest forecast-outcome surprises and category Brier | Measurement | S | zero |
| T9 devil's advocate | **Weak — defer** | We already force `COUNTERCASE` in the same call (self-critique). An independent stress-test call duplicates T2's benefit for more money | marginal | M | +1 call/analysis — not worth it before T2 is measured |
| T6 caps/cluster risk | **Out of scope now** | Kelly loop explicitly out of scope (FV-drift). Note for later: no cross-market correlated-cluster cap exists in either sizer | risk ↓ | L | zero |
| T2/T6 approve-reject limit loop | **No** | Our sizers are mechanical by design; re-inserting an LLM approval step reverses SYB-210 and costs tokens | — | — | — |
| T4 "sell at 70% against" | **No (already solved)** | Belief-action gap doesn't exist here: sizers rebalance to FV targets mechanically every 30-60s. terminator2 needed this rule because a *hand* had to click sell | — | — | — |
| T1 catalog scanners / Vegas de-vig | **No** | Domain-specific to AI-release/sports books; our cross-platform anchor is the Polymarket mirror price, already in every prompt | — | — | — |
| 20-min Opus agentic loop | **No** | One cycle ≈ our analyst's lifetime budget | — | — | — |

## 4. Staged implementation plan (ROI order)

**Stage 0 — measurement (complete 2026-07-12):** `S`, zero spend
1. **Outcome ground truth — done 2026-07-12**: active Stage 1 experiments immediately and
   periodically query Sybil's authoritative resolution endpoint for exactly their immutable cohort
   and write `market_outcomes` (market_id, outcome, resolved_at). Writes use the live WAL/busy
   timeout policy, conflicts fail closed, and transient network/SQLite failures retry without
   interrupting trading. `scripts.record_outcomes` remains the manual entry point.
2. **Rejection reasons — done**: `trader.py` assigns a reason to every no-order sizing decision;
   `live/db.py` requires and persists it together with market category/tags.
3. **Calibration additions — done**: `calibration.py` reports per-reason rejection
   counterfactuals, category Brier, surprises, and pinned half-open windows/cohorts.

The implementation criterion is met. The separate experiment evidence criterion is a strict
24-hour concurrent report once authoritative outcomes exist for the frozen cohort.

**Stage 1 — prompt scaffolding (implemented; measurement pending):** `S`, +2-4% tokens
- `RESTATE:` resolution-criteria paraphrases are required, parsed, and logged.
- Prompts identify each article source and instruct the model to discount aggregator/SEO summaries.
- The opt-in concurrent runner compares the pre-Stage-1 control with the current Stage-1 contract
  over paired evidence batches and isolated Flat accounts.

Run the strict report with `scripts/calibration.py --experiment-id <id> --until <exclusive-end>`.
It derives the persisted UTC start and exact cohort from `live_experiments`; matched-batch Brier,
parse-fallback rate, spend, and Flat PnL are the primary checks.

**Stage 2 — price-move re-estimation trigger (T1):** `M`, +20-50% analyst calls (budget-gated)
- In `analyst.py::on_block`, alongside news drain: if `polymarket_prices.get_price(mid)` moved
  >θ (start θ=0.10) from the price at last FV publish and FV age > ttl, run the LLM with a
  "price moved, no new articles — re-derive" prompt variant. Respect `min_llm_interval_s`
  and the SYB-64 pause threshold (which blocks subsequent calls after crossing).
Measure: stale-FV losses (decisions where fair_value_age_s > ttl at order time) and PnL delta.

**Stage 3 — oracle second-opinion (T2):** `M`, +5-20% spend depending on gating
- Edge-triggered: when the analyst's new FV differs from its prior *or* from market by >10pp,
  call a second model (different family) with a minimal prompt (question + resolution criteria +
  price, **no articles**) for an independent base-rate estimate. If |FV−oracle| >15pp: publish
  with confidence×0.5 and log a `challenge` row; also track oracle failures (our
  `oracle_accuracy` analogue) so we can fire it if it's noise.
Measure: Brier of challenged vs unchallenged decisions; confidence-weighted reliability.

**Stage 4 — mistakes ledger → self-rules (T4):** `M`, negligible spend
- After each calibration window: top-N surprises with submitted orders → `mistakes` table →
  3-5 persona-specific rules appended to the persona text (persona prompt injection point
  already exists: `analyst.py::_build_prompt` `{self.persona}`). Manual curation first;
  automation only if window-over-window Brier improves.

**Stage 5 — later/optional:** per-persona statistical recalibration (temperature scaling of
logged FVs — *our* idea, not terminator2's; needs weeks of resolved data), correlated-cluster
exposure caps for the Flat arm, Flat-arm confidence usage.

## 5. Measurement protocol (before/after, ticket Done criterion)

- **Shared market set**: freeze the selected market-id list at experiment start (runner
  `--market-ids`); all variants trade exactly that set. Pass the same comma-separated list to
  `calibration.py --market-ids` so forecast scoring cannot drift to a different cohort. This
  filter does not reconstruct per-market PnL: portfolio PnL remains the whole trader account,
  so the runner-level cohort pin is still required for the PnL comparison.
- **A/B harness already exists**: each persona's control/variant analysts consume the same
  per-market article list and snapped reference price through a paired batch barrier, then publish
  onto separate buses for separate Flat sizers/accounts. The next feed batch stays blocked until
  both arms consume the active one. Flat is the primary PnL readout (Kelly is out of scope and
  higher-variance).
- **Metrics per window** (calibration.py): per-analyst Brier vs market-price baseline (the
  delta column), reliability curve, rejection accuracy (per reason, Stage 0), Flat-arm PnL vs
  NativeNoiseTrader baseline, LLM $/decision from `token_usage`. When `--since` is supplied,
  portfolio PnL is reported as the per-trader delta from the last pre-window snapshot to the
  last snapshot inside the half-open window; without it, the report keeps cumulative PnL.
  Cross-window deltas match the exact intersection of durable trader names per arm and fail if
  Flat has no matched accounts; added/removed identities are reported rather than averaged in.
- **Windows**: ≥1 day per side, or concurrent A/B (preferred — same news, same prices). Persisted
  `analysis_batch_id` values bind market + sorted URLs + snapped price, de-duplicate repeated sizer
  rows, and expose unmatched arm batches. Primary Brier/baseline deltas use only the exact matched
  batch intersection; full-arm metrics are diagnostic. Experiment ids cannot resume; any restart
  requires a new window/id.
- **Persisted concurrent report**: run `calibration.py --experiment-id <id> --until <exclusive
  ISO-UTC>`. It derives the start/cohort from `live_experiments`, fingerprint-checks an exact
  analyst/Flat identity allowlist, and reports calls, USD, cost per decision/batch, matched-batch
  coverage, spend deltas, and Flat PnL. It refuses windows under 24 hours unless
  `--exploratory-short-window` is explicitly supplied; the labeled text/JSON override cannot meet
  the Done criterion. A strict report also requires every exact Flat arm's first/latest portfolio
  snapshots to fall within ten minutes of the persisted start/exclusive end and its maximum
  consecutive gap to stay at or below ten minutes (normal cadence: 300 seconds). The report records
  each arm's endpoints, maximum gap, and coverage status; incomplete coverage fails strict mode and
  is only retained with the exploratory label. Strict experiment Brier uses explicit outcomes only,
  filtered to the frozen cohort before source/count reporting, while spend and PnL remain
  meaningful before resolution. The exclusive end may not be in the future; snapshot endpoint
  tolerance never authorizes future evidence.
- **Guardrail**: any stage that raises analyst spend must show cost per decision; abort a stage
  if projected spend exceeds 2× baseline. Configured thresholds are $5–10, not hard ceilings;
  each analyst may overshoot by one completed call.

## 6. OpenRouter spend impact summary

| Stage | Extra calls | Est. spend delta per analyst |
|-------|-------------|------------------------------|
| 0 | none | $0 |
| 1 | none (+~40 output tok/call) | +2-4% |
| 2 | price-triggered re-estimates | +20-50% (θ- and threshold-gated) |
| 3 | edge-triggered oracle (cheap model) | +5-20% |
| 4 | none (+~100 prompt tok) | +1-2% |
| A/B harness | duplicate analyst per experiment | ×2 during experiment windows only |

Baseline reference: deepseek-v4-flash analyst calls are ~$0.001-class; the $5 default pause
threshold (`runner.py --llm-budget-usd`) blocks later calls after it is reached. One completed call
can cross the threshold before its billed cost is known.

## 7. What surprised me

1. terminator2 does **no statistical calibration post-processing** — its Brier 0.027 comes from
   behavioral machinery (oracle challenges, self-rules, rejection discipline) plus a longshot-
   heavy market diet. The transferable core is *deterministic scaffolding around one LLM call*,
   which fits our budget model perfectly.
2. Its single most expensive lesson (M$429, "detection without action is expensive theater") is
   one we've **already solved architecturally** with mechanical sizers — our Kelly/Flat split is
   ahead of terminator2 on that axis.
3. Their rejection counterfactuals quantify rule cost: the concentration cap "insurance" forwent
   1,329 profitable trades of 1,520 rejected. We now have the same lens: every no-order decision
   records a reason and calibration scores the counterfactual by reason.
4. Our own `db.py` already carries SYB-114-tagged calibration columns (raw/effective FV,
   confidence, countercase) — someone pre-wired the measurement; the missing pieces are only
   outcome labels and rejection reasons.
