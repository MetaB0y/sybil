# The Arena (Python)

**Directory:** `arena/` — live runner, SDK, bots, sim, per-market configs, nba, dashboards

## Verdict

The arena's live layer has a genuinely nice core idea — the LLM produces fair values, and pluggable mechanical strategies (Kelly vs Flat) do the sizing — and its pure modules (`strategy.py`, `market_selection.py`) are clean and well-tested. But the live runner has a **critical fan-out bug that invalidates the whole Kelly-vs-Flat experiment**, prices used for sizing freeze at startup, a currently-failing date test exposes an expiry heuristic that overrides the API, and any single bot exception tears down the entire process and abandons all accounts. Around this live core sit ~9k lines of dead or duplicated material (three generations of experiments). This is the subsystem with the highest deletion-to-value ratio in the repo.

## Architecture as built

Three generations coexist; only the newest is deployed:

- **Gen 1 (dead):** `arena/nba/` — 6.6k lines of a self-contained sports-trading backtest stack (its own framework, bots, TUI, ESPN scraper). Dead in four independent ways: it breaks bare `pytest` collection, its TUI references a deleted CSS file, its examples import symbols that no longer exist, and its datasets are gone. Nothing outside `nba/` imports it.
- **Gen 2 (research artifacts):** `markets/` (per-market persona/source configs — iran, china_visit, texas_primary), `sim/` (time-compressed replay harness), `viz/news_explorer.py` (a 1,774-line Streamlit app that replays runs by **re-parsing human-readable order strings** to reconstruct matching state client-side), and root-level `mm_*.py` scripts (1,742 lines pinned to gitignored data that exists on no other machine).
- **Gen 3 (deployed):** `live/` — the runner loop. `runner.py` discovers markets once, creates 6 LLM traders (3 personas × Kelly/Flat) + noise bots, wires them to **one shared `NewsFeed`**, and spawns one asyncio task per bot under an `asyncio.wait(FIRST_COMPLETED)` that treats any task exit as fatal. `trader.py` (`LiveLlmTrader`) drains articles per market, calls the LLM for a `FAIR_VALUE`, and separately runs the mechanical strategy. `strategy.py` has a clean `KellyStrategy`/`FlatStrategy` split over pure functions. `news_feed.py` polls per-market Google News RSS, gates via a cheap LLM, and also owns `PolymarketPrices` (loads the mirror's mapping JSON). `metrics.py` is a hand-rolled Prometheus exporter; `db.py` a WAL-mode SQLite decision log; `dashboard.py`/`queries.py` a Streamlit monitor with a shared query layer.

**Doc drift:** the vault's Bot Framework / LLM Trader / Python SDK notes and `arena/AGENTS.md` describe the sim/markets/nba world and **omit `live/` entirely** — the only deployed and actively developed subsystem. The SDK doc claims signed-order support the SDK does not have.

## Strengths

- **Clean LLM/sizing separation:** the LLM emits only `FAIR_VALUE`; the strategies are pure functions of `(fv, price, positions)` and thoroughly unit-tested.
- `market_selection.py` is pure, typed, and testable — profile logic isolated from the runner.
- **Observability is unusually good** for a bot sandbox: dependency-free Prometheus exporter seeded from SQLite, vmalert rules for feed/decision liveness, an FV-divergence ("conviction loop") monitor in both the dashboard and CLI.
- The `live/queries.py` pattern (queries return DataFrames; rendering is the caller's job, shared between Streamlit and a CLI) is the right dashboard architecture.
- Sensible operational hardening: bounded RSS fan-out with error aggregation, warm-start to skip startup backlog, IOC default to avoid stale resting orders.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [H10](01-critical-bugs.md) | bug | **critical** | All six LLM traders share one destructive `NewsFeed` queue → each article reaches exactly one trader → the Kelly-vs-Flat comparison is invalid |
| [H9](01-critical-bugs.md) | bug | high | Malformed `FAIR_VALUE` (trailing period) raises `ValueError` → kills the bot task → runner kills the whole arena and orphans all portfolios |
| AR-1 | bug | high | Market prices used for sizing are frozen at process startup (`reference_price_nanos` from the initial snapshot always wins over the fresh Polymarket poll); the LLM sees a different price than the sizing engine |
| AR-2 | bug | high | Title-date expiry heuristic overrides authoritative API expiry and assumes the current year → excludes live markets; a date-dependent test is **currently failing** (`test_selection_skips_expired_markets`) |
| AR-3 | design | high | Restart abandons portfolios: accounts are re-created every process start (no persisted ids), and any transient SSE drop (no reconnect) triggers a full restart → orphaned accounts + reset leaderboards + re-minted cash |
| AR-4 | bug | high | `FlatStrategy` "hard exit" keys on absolute price level, not adverse movement, with no cost basis → perpetual buy/sell churn on any market priced <0.30 or >0.70 |
| AR-5 | ops | medium | Deployed stack disables fill history (`SYBIL_MAX_RECENT_FILLS_PER_ACCOUNT=0`) → `on_fill`/`total_fills` permanently dead; offset pagination breaks under trimming anyway |
| AR-6 | bug | medium | `min_llm_interval` gates loop entry, not calls → one block can fire an unbounded burst of sequential LLM calls per trader |
| AR-7 | bug | medium | Global URL dedup assigns a multi-market article to only the first market's feed (`matched_market_ids` is always a singleton) |
| AR-8 | bug | medium | `IMPORTANT_NEWS_TERMS` matched by substring, not word boundary ("war" matches "warriors", "ban" matches "Taliban") |
| AR-9 | ops | medium | Dev compose can never load the Polymarket mapping (default path points at the wrong volume) → live mids silently absent in the dev stack |
| AR-10 | bloat | high | ~9k LOC legacy/duplicate: `nba/` (6.6k, breaks `pytest`), `news_trader_legacy`, root `mm_*` scripts, `composition_demo` (1.7MB tracked JSON with deleted generators), 4 of 6 unused MM classes |
| AR-11 | bloat | medium | `sim/` and `live/` duplicate the whole LLM-trader substrate (FV parsing, prompt building, client factory) with drift |
| AR-12 | bug | high | `just arena-demo` runs a `demo.py` moved into `nba/` four months ago (and whose datasets are deleted) — broken entry point advertised in AGENTS.md |
| AR-13 | design | medium | `sim/results.py` serializes orders as display strings; four downstream consumers re-parse them (root cause of ~500 lines of brittle reconstruction) |
| AR-14 | inconsistency | medium | Config sprawl: public server IP hardcoded in multiple defaults; every `LiveConfig` default duplicated as an argparse default; model ids repeated in 3+ places |
| AR-15 | doc-drift | medium | SDK has dead compat shims, no retries/reconnect, and the vault claims signing support it lacks; vault + AGENTS.md omit `live/` and carry a phantom 5-block TTL |
| AR-16 | test-gap | medium | Zero tests for `NewsFeed`, runner orchestration, metrics, or DecisionDB; the live trader has 4 shallow tests — none exercise the paths where the critical bugs live |
| AR-17 | inconsistency | low | `print` vs `logging`, naive vs aware datetimes, SQL f-string interpolation, swallowed exceptions, duplicate `NANOS` constant, deprecated ruff config, `int()` truncation losing 1 nano |

## Ambitious ideas

1. **Split analysis from trading** (fixes H10 by construction): an `AnalystService` makes one LLM call per `(article-batch, market, persona)` and publishes a `FairValueUpdate` event; Kelly and Flat become pure subscribers of the same FV stream. This halves LLM cost, makes the A/B comparison scientifically valid (identical inputs, differing only in sizing), and mirrors the repo's actor convention.
2. **Unify `sim/` and `live/` into one trader core** with injected boundaries: a `Clock` protocol (sim | wall), an `ArticleSource` (dataset | NewsFeed), and a `PriceSource` (clearing | Polymarket). Backtests then exercise the exact code that trades live — the highest-leverage correctness move for the subsystem.
3. **Delete a third of the arena in one commit:** `nba/` (6.6k), `feeds/`, the `mm_*` scripts, `news_trader_legacy`, `composition_demo`, and the broken `arena-demo` targets. The survivor set (sybil_client, bots, sim, markets, live, viz, tests) is coherent and fully described by a rewritten `AGENTS.md`.
4. **Make bot identity durable:** persist `(persona, strategy) → account_id`, reattach on startup, add reconnecting `stream_blocks`, and switch the runner to per-task supervision with backoff. Restarts become non-events and long-horizon PnL becomes measurable — the precondition for the arena to be a real evaluation harness (fixes H3, H9).
5. **Replace the regex/keyword market profile with the LLM gate the arena already pays for:** classify each candidate market once, cache verdicts in SQLite, and keep only expiry/volume/diversity in code — killing ~120 lines of brittle pattern lists and the substring-match bug class (AR-8).
6. **Generate the SDK types from `sybil-api`'s OpenAPI spec** so SDK/API drift (the signed-orders doc lie, missing endpoints) becomes structurally impossible.
7. **Make `sim/results.py` emit a structured schema** (order/fill records with nanos, side enums, source, block) and turn `news_explorer.py` into a thin renderer instead of a client-side matching-engine reimplementation — deleting ~500 lines of the riskiest code (AR-13).
