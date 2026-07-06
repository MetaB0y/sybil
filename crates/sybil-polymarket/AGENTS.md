# AGENTS.md

This file provides guidance to Claude Code when working with code in this crate.

## Purpose

The **sybil-polymarket** crate mirrors Polymarket markets onto a Sybil instance and runs a reference-price market maker. It treats sybil-api as a blackbox — pure HTTP client, no sybil workspace crate imports.

## Architecture

Four tokio actors communicate via channels:

```
SyncActor ──→ FeedActor:  mpsc (new token subscriptions)
SyncActor ──→ MmActor:    mpsc (new market notifications)
FeedActor ──→ MmActor:    watch (real-time price snapshots)
ResolutionActor polls Gamma and resolves Sybil markets via service API calls
```

### SyncActor (`sync.rs`)
Polls Polymarket Gamma API for new events, creates corresponding markets/groups on Sybil, maintains the mapping store.

### FeedActor (`feed.rs`)
WebSocket connection to Polymarket CLOB for real-time prices. Falls back to REST polling. Publishes `PriceSnapshot` via watch channel.

### MmActor (`mm.rs`)
Listens to the Sybil WebSocket block stream with `?from_block=` resume. Each block: reads latest Polymarket reference price, submits BuyYes + BuyNo as flash liquidity (mm_budget_nanos).

### ResolutionActor (`resolution.rs`)
Polls mirrored Polymarket markets for resolution status and submits signed/authorized Sybil resolutions when outcomes settle.

### AutoResolveActor (`autoresolve.rs`, SYB-48)
LLM auto-resolution for **native** markets (the checked-in catalog, `native.rs`)
whose `resolution_source` is `api_poll`. For each such market past its
`end_time` it fetches the endpoint (plain `reqwest`, bounded timeout, per-source
rate limit), sends the fetched content plus the market's **full**
`resolution_criteria` to an LLM (`llm.rs`), and parses STRICT JSON **fail-closed**
(garbled output escalates, never resolves). A confidence policy then routes it:

- **≥ `confidence_propose`** (default 0.9): sign a resolution attestation and
  hold it in a **resolver-side** pending queue for a **challenge window**
  (default 24h). The proposal is posted to sybil-api's review board so operators
  can **reject** (durable veto) or **approve** (finalize early). When the window
  elapses with no veto, the resolver replays the signed attestation through the
  **existing** `resolve_market_attested` money path — nothing bypasses the
  oracle guards.
- **≥ `confidence_review`** (default 0.7): review queue only. No attestation,
  nothing auto-finalizes.
- below that / any parse or fetch failure: escalate.

`manual` sources are never touched (operator workflow). `deepseek/deepseek-v4-flash`
by default; key from `OPENROUTER_API_KEY` (same var the arena uses).

**Disabled by default.** Requires `AUTORESOLVE_ENABLED=true` **and** a
`SIGNER_KEY_PATH` **and** `OPENROUTER_API_KEY` **and** a loaded native catalog;
any missing → the actor stays a no-op.

Review board (service-gated on sybil-api):
`GET/POST /v1/admin/auto-resolutions`, `POST /v1/admin/auto-resolutions/{id}/approve`,
`POST /v1/admin/auto-resolutions/{id}/reject`.

Tuning env vars: `AUTORESOLVE_POLL_INTERVAL_SECS`, `AUTORESOLVE_CONFIDENCE_PROPOSE`,
`AUTORESOLVE_CONFIDENCE_REVIEW`, `AUTORESOLVE_CHALLENGE_WINDOW_HOURS`,
`AUTORESOLVE_SOURCE_MIN_INTERVAL_SECS`, `AUTORESOLVE_MODEL`.

## Module Map

| Module | Purpose |
|--------|---------|
| `config.rs` | CLI + env configuration (clap) |
| `error.rs` | Crate-level error types |
| `mapping.rs` | Bidirectional Polymarket <-> Sybil ID mapping |
| `polymarket/types.rs` | Gamma event/market types, WS message types |
| `polymarket/gamma.rs` | Gamma REST client, CLOB midpoint client |
| `polymarket/ws.rs` | CLOB WebSocket price feed |
| `sybil/client.rs` | Legacy Sybil HTTP client surface; shared Rust client lives in `sybil-client` |
| `sync.rs` | SyncActor |
| `feed.rs` | FeedActor |
| `mm.rs` | MmActor |
| `resolution.rs` | ResolutionActor |
| `autoresolve.rs` | AutoResolveActor (SYB-48 LLM auto-resolution) |
| `llm.rs` | LlmClient trait + OpenRouter impl + deterministic MockLlm |
| `main.rs` | Orchestration + shutdown |

## Running

```bash
# Start sybil-api first
cargo run --release -p sybil-api -- --dev-mode --port 3000

# In another terminal
cargo run --release -p sybil-polymarket -- --sybil-url http://localhost:3000 --max-events 10
```

## Key Design Decisions

- **No sybil crate imports**: This crate is a standalone HTTP client. Sybil DTOs are duplicated in `sybil/types.rs`.
- **Watch channel for prices**: Single-writer (FeedActor), multiple-reader pattern. Always has the latest snapshot.
- **Flash liquidity**: MM submits all orders with `mm_budget_nanos` — the solver picks the welfare-optimal subset.
- **WebSocket reconnect**: Proactive reconnect every 15 minutes to preempt Polymarket's known 18-22 minute zombie connection bug.
- **Mapping persistence**: Optional JSON file. In-memory by default for dev.

## Curated mirror seed set (SYB-150)

For a hand-picked launch the mirror runs in **curated mode** instead of the
broad volume-ranked scan. Set `--curated-markets-path` /
`CURATED_MARKETS_PATH` to `curated_markets.json` and the `SyncActor` mirrors
**only** the events listed there, addressed by Polymarket Gamma **event id**
(deterministic, auditable). The deploy wires this via a read-only bind mount
(`docker-compose.yml` → `/etc/sybil/curated_markets.json`). A parse error at
startup is fatal, so a typo cannot silently fall back to the broad scan.

Loader + validation: `src/curated.rs` (unit-tested, incl. an `include_str!`
check of the checked-in file). Gamma fetch: `GammaClient::fetch_curated_events`
(`src/polymarket/gamma.rs`) — repeated `?id=` params, keeps only events with a
tradeable (active, non-closed) child market.

### The set (verified live on Gamma 2026-07-05)

| Event id | Slug | Shape | Ends |
|---|---|---|---|
| 556382 | which-company-has-best-ai-model-end-of-2026 | NegRisk group (15 active co. legs) | 2026-12-31 |
| 333737 | will-a-chinese-company-have-the-best-ai-model-by-december-31 | Single binary | 2026-12-31 |
| 206787 | which-companys-ai-will-first-hit-1550-on-chatbot-arena-in-2026 | NegRisk group (~11 legs + "None") | 2026-12-31 |
| 79075 | us-enacts-ai-safety-bill-before-2027 | Single binary | 2026-12-31 |
| 85299 | ai-bubble-burst-by | Multi (only the 2026 leg is live) | 2026-12-31 |
| 96557 | will-ai-be-charged-with-a-crime-before-2027 | Single binary | 2026-12-31 |
| 192873 | what-kind-of-product-will-openai-announce-in-2026 | Multi (10 product binaries) | 2026-12-31 |
| 83771 | which-ceos-will-be-out-before-2027 | Multi (6 CEO binaries; Tim Cook leg closed) | 2026-12-31 |
| 500753 | will-anthropics-valuation-hit-by-december-31 | Threshold ladder (13 binaries) | 2027-01-01 |
| 500775 | will-openais-valuation-hit-by-december-31 | Threshold ladder (15 binaries) | 2027-01-01 |

All ten ticket candidates are live; none are unavailable. "AI bubble burst"
maps to the event's single active leg ("AI bubble burst in 2026?",
groupItemTitle "December 31, 2026"); its March-2026 and Dec-2025 legs are
already closed and are dropped by the active/!closed gate.

### Valuation-threshold documentation (ticket "configured threshold")

The two valuation candidates are threshold **ladders**, not single markets. The
mirror mirrors the whole live ladder as a frontend multi-card. Per the ticket
we also pin and document the *configured/at-the-money* threshold — the live
market closest to 50/50 as of 2026-07-05:

- **Anthropic** (event 500753): **HIGH $1.75T** — condition
  `0xc68fea7daba83292fdd7bb704f26d38c4f678cbc2ff69c4175d47c55e74b4321`
  (YES ≈ 0.475). The $1.0T / $1.1T legs are already resolved YES (closed).
- **OpenAI** (event 500775): **HIGH $1.25T** — condition
  `0x0a0fb756d8dc5c3cf0a16eebc7d6d39406155a41e13d00bf615fc0fcd9e9c31a`
  (YES ≈ 0.485).

Both ladders resolve `2027-01-01T12:00:00Z` (labelled "by December 31").

### Provenance (mirror-with-source)

No new metadata field is needed — the existing `set_market_metadata` path
already marks each mirrored market end to end. `build_metadata_request`
(`src/sync.rs`) sets `polymarket_condition_id`, `event_id` + `event_title`, and
`external_url` (`https://polymarket.com/event/{slug}`, the "view on Polymarket"
resolution link). Those flow mirror → sequencer `MarketRefData` → API
`MarketResponse` (`polymarket_condition_id` / `event_id` / `external_url`).
`polymarket_condition_id` being non-null **is** the mirror marker a frontend
badge/filter reads; the arena also gets a live Polymarket reference via
`reference_price_nanos` pushed by `MmActor::set_reference_prices`.

### Updating the set

Verify an event is still tradeable —
`curl 'https://gamma-api.polymarket.com/events?id=<event_id>'` (`active:true`,
`closed:false`, ≥1 active child) — then add/remove an entry in
`curated_markets.json` and redeploy sybil-polymarket. The `curated.rs` unit
tests keep the file parseable and honest.

## Polymarket API Notes

- Gamma API: `gamma-api.polymarket.com` — no auth needed, 4000 req/10s
- CLOB WebSocket: `wss://ws-subscriptions-clob.polymarket.com/ws/market` — no auth, needs PING every 10s
- CLOB REST: `clob.polymarket.com` — no auth, 9000 req/10s
- `outcomes`, `outcomePrices`, `clobTokenIds` fields are JSON strings inside JSON (double-parse)
- Token IDs are 77+ digit integers stored as strings
- NegRisk events = multi-outcome → map to Sybil MarketGroups

## Testing

```bash
cargo test -p sybil-polymarket
cargo clippy -p sybil-polymarket
```
