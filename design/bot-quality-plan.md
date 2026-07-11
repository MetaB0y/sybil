---
tags: [arena, bots, calibration, research, terminator2]
status: proposal-needs-revalidation
date: 2026-07-10
ticket: SYB-114
---

# SYB-114 research: terminator2-agent techniques for the live arena bots

> **Status note (2026-07-11):** research input, not the current arena backlog.
> Resurvey the live bot pipeline and calibration data before executing a stage.

Implementation is gated on the first calibration window (Valery runs `scripts/calibration.py`
after ~a day of post-genesis trading; genesis was 2026-07-10, so data lands 2026-07-11).
This note is the research half + staged plan.

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
  budget **$5/analyst default** (`llm_budget_usd`), hard pause at $0 (SYB-64); actual billed
  cost via `usage.include` (`live/costs.py`). Min 60s between LLM calls (`min_llm_interval_s`).
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
(JSON), motivation, analysis, raw_llm_response, balance, positions, article_urls. Token spend
per call in `token_usage`. Portfolio snapshots every 300s.

**Calibration measurement today** (`scripts/calibration.py`): per-persona Brier vs
market-price-as-forecast baseline, reliability curve, rejection calibration
(rejected = decision row with no orders), NativeNoiseTrader PnL baseline. **Two gaps:**
1. Outcomes are **inferred from last observed decision price ≥0.95 / ≤0.05** unless an explicit
   `market_outcomes` table exists — nobody writes that table today, so labels are noisy and
   censored (unresolved markets silently dropped).
2. "Rejected" conflates *why*: below-min-edge, stale FV, resolved market, budget-paused, and
   no-cash all look identical (orders=[]). No rejection reasons are logged.

So: per-bot calibration comparison is *possible* tomorrow, but on inferred labels and without
reject-reason attribution. That defines Stage 0.

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

Setting constraints: 10s blocks, EG/Fisher batch clearing, IOC orders, $5–10 hard-capped
OpenRouter keys, deepseek-v4-flash analysts, continuous operation, Kelly sizing loop OUT of
scope (known FV-drift concern).

| T# | Transfers? | Our version | Expected effect | Cost | Spend impact |
|----|-----------|-------------|-----------------|------|--------------|
| T5 rejection reasons + counterfactuals | **Yes — first** | Add `rejection_reason` to sizer decision rows; extend `calibration.py` per-reason would-have-profited | Measurement (enables Done criterion); indirectly PnL via tuning min_edge | S | zero |
| T1 price-move trigger | **Yes — highest PnL** | Analyst currently re-estimates *only on news*. Add: if Polymarket ref moved >8-10pp since last FV (cache already in `news_feed.PolymarketPrices`), enqueue a re-estimate even without articles. Directly attacks stale-FV / FV-drift | PnL + variance (fewer stale-FV losses); modest calibration | M | +20-50% analyst calls, bounded by existing 60s interval + budget pause; gate threshold conservatively |
| T2 oracle second-opinion | **Yes** | Second cheap model (reuse gate-class `google/gemma-4-31b-it` or another flash model, different family than deepseek) gives independent FV; if gap >15pp → clamp confidence (e.g. ×0.5) or hold prior. Log both estimates | Calibration (overconfidence catch), variance ↓ | M | +1 cheap call per analysis ≈ +10-20% cost; or edge-triggered only (call oracle only when analyst wants to move FV >10pp) ≈ +5% |
| T4 self-rules ledger | **Yes (semi-manual)** | `mistakes` table fed from calibration "surprises" (biggest forecast-outcome gaps with orders); per-persona rules appended to persona prompt after each calibration window. Start manual (Valery/agent writes 3-5 rules from window 1), automate later | Calibration + PnL, slow compounding | M | negligible (+~100 prompt tokens) |
| T3 comprehension guard | **Yes (prompt-only)** | Add required `RESTATE:` field — one-line paraphrase of what resolves YES — before FAIR_VALUE; parse and log it. Directly targets terminator2's question_comprehension_failure class | Calibration on ambiguous markets | S | +~40 output tokens/call (~2-4%) |
| T8 source-tier discipline | **Partial** | Prompt already ranks "official actions > quotes > analysis > speculation" (`analyst.py::SYSTEM_PROMPT`). Add source domain tier (official/wire/aggregator/blog) computed in `news_feed.py` and injected per article | Calibration (fabricated/SEO news discount) | M | zero |
| T7 surprises list | **Yes** | Add "surprises" (top-N gap) and by-category Brier to `calibration.py` output | Measurement | S | zero |
| T9 devil's advocate | **Weak — defer** | We already force `COUNTERCASE` in the same call (self-critique). An independent stress-test call duplicates T2's benefit for more money | marginal | M | +1 call/analysis — not worth it before T2 is measured |
| T6 caps/cluster risk | **Out of scope now** | Kelly loop explicitly out of scope (FV-drift). Note for later: no cross-market correlated-cluster cap exists in either sizer | risk ↓ | L | zero |
| T2/T6 approve-reject limit loop | **No** | Our sizers are mechanical by design; re-inserting an LLM approval step reverses SYB-210 and costs tokens | — | — | — |
| T4 "sell at 70% against" | **No (already solved)** | Belief-action gap doesn't exist here: sizers rebalance to FV targets mechanically every 30-60s. terminator2 needed this rule because a *hand* had to click sell | — | — | — |
| T1 catalog scanners / Vegas de-vig | **No** | Domain-specific to AI-release/sports books; our cross-platform anchor is the Polymarket mirror price, already in every prompt | — | — | — |
| 20-min Opus agentic loop | **No** | One cycle ≈ our analyst's lifetime budget | — | — | — |

## 4. Staged implementation plan (ROI order)

**Stage 0 — measurement (prereq, do before any technique):** `S`, zero spend
1. **Outcome ground truth**: write a `market_outcomes` table (market_id, outcome, resolved_at)
   from Polymarket resolutions via the existing mapping file / sybil market status; `calibration.py`
   already prefers explicit outcomes (`_load_explicit_outcomes`) — today nothing populates it.
   Small standalone script or a runner task.
2. **Rejection reasons**: thread a `rejection_reason` string through
   `trader.py::_rebalance_all` → `_record_trade` → new `decisions` column (schema migration
   pattern already exists in `db.py::_create_tables`). Reasons: below_min_edge, fv_expired,
   resolved, insufficient_cash, budget_paused (analyst side), hold_position.
3. **calibration.py additions**: per-reason rejection counterfactuals, surprises list,
   by-category Brier (market tags exist), and a `--since`/`--until` window filter so before/after
   windows are clean.
Done-when: tomorrow's calibration run produces per-persona Brier on explicit outcomes with
per-reason rejection stats.

**Stage 1 — prompt scaffolding (T3 + T8-lite):** `S`, +2-4% tokens
- Add `RESTATE:` field (resolution-criteria paraphrase) to the analyst format; log it.
- Add per-article source line + one prompt sentence discounting aggregator/SEO sources.
Measure: Brier delta on shared market set; parse-fallback rate must not rise.

**Stage 2 — price-move re-estimation trigger (T1):** `M`, +20-50% analyst calls (budget-gated)
- In `analyst.py::on_block`, alongside news drain: if `polymarket_prices.get_price(mid)` moved
  >θ (start θ=0.10) from the price at last FV publish and FV age > ttl, run the LLM with a
  "price moved, no new articles — re-derive" prompt variant. Respect `min_llm_interval_s`
  and the SYB-64 budget (it already hard-caps worst case).
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
  `--market-ids`); all variants trade exactly that set.
- **A/B harness already exists**: the FairValueBus fan-out (SYB-192/210) guarantees identical
  news inputs. For each experiment, run the *same persona* twice — control analyst vs variant
  analyst (technique on) — each with its own bus + Flat sizer, same feed. Flat arm is the
  primary PnL readout (Kelly out of scope and higher-variance).
- **Metrics per window** (calibration.py): per-analyst Brier vs market-price baseline (the
  delta column), reliability curve, rejection accuracy (per reason, Stage 0), Flat-arm PnL vs
  NativeNoiseTrader baseline, LLM $/decision from `token_usage`. When `--since` is supplied,
  portfolio PnL is reported as the per-trader delta from the last pre-window snapshot to the
  last snapshot inside the half-open window; without it, the report keeps cumulative PnL.
- **Windows**: ≥1 day per side, or concurrent A/B (preferred — same news, same prices). Compare
  on decisions matched by (market_id, article batch) where possible.
- **Guardrail**: any stage that raises analyst spend must show cost per decision; abort a stage
  if projected spend exceeds 2× baseline (keys hard-capped $5–10).

## 6. OpenRouter spend impact summary

| Stage | Extra calls | Est. spend delta per analyst |
|-------|-------------|------------------------------|
| 0 | none | $0 |
| 1 | none (+~40 output tok/call) | +2-4% |
| 2 | price-triggered re-estimates | +20-50% (θ- and budget-capped) |
| 3 | edge-triggered oracle (cheap model) | +5-20% |
| 4 | none (+~100 prompt tok) | +1-2% |
| A/B harness | duplicate analyst per experiment | ×2 during experiment windows only |

Baseline reference: deepseek-v4-flash analyst calls are ~$0.001-class; the $5 default budget
(`runner.py --llm-budget-usd`) already hard-pauses overruns, so worst case is analysts pausing
earlier, not overage.

## 7. What surprised me

1. terminator2 does **no statistical calibration post-processing** — its Brier 0.027 comes from
   behavioral machinery (oracle challenges, self-rules, rejection discipline) plus a longshot-
   heavy market diet. The transferable core is *deterministic scaffolding around one LLM call*,
   which fits our budget model perfectly.
2. Its single most expensive lesson (M$429, "detection without action is expensive theater") is
   one we've **already solved architecturally** with mechanical sizers — our Kelly/Flat split is
   ahead of terminator2 on that axis.
3. Their rejection counterfactuals quantify rule cost: the concentration cap "insurance" forwent
   1,329 profitable trades of 1,520 rejected. We can get the same lens nearly for free
   (Stage 0.2) because our decisions table already logs no-order decisions.
4. Our own `db.py` already carries SYB-114-tagged calibration columns (raw/effective FV,
   confidence, countercase) — someone pre-wired the measurement; the missing pieces are only
   outcome labels and rejection reasons.
