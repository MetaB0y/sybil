# Polymarket Mirror

**Crate:** `sybil-polymarket` (~3k LOC)

## Verdict

One of the better-factored leaf crates — four tokio actors, pure quoting/parsing functions with real tests, deliberate schema-drift tolerance — with three serious correctness holes in its untested state machines, plus a leaky cross-container JSON file that doubles as an API to the arena.

## Architecture as built

Four actors wired by channels, with `main.rs` orchestrating startup and a `select!` that turns any actor panic into process shutdown:

- **SyncActor** (`sync.rs`, 469): every 120s fetches the top-50 active Gamma events by volume (category/volume filtered), PUTs event JSON to a dev-mode endpoint, creates sybil markets for unmapped active markets (name `"EventTitle: GroupItemTitle"`), registers the mapping, posts metadata, sends `MarketMirrored` to the MM (capped by `mm_max_markets`), batches YES token ids to the feed, and creates a `MarketGroup` for NegRisk events. Saves the mapping JSON (non-atomic `fs::write`) each cycle.
- **FeedActor** (`feed.rs` + `polymarket/ws.rs`): subscribes to the Polymarket CLOB WebSocket for all tokens, parses midpoints into a `watch<PriceSnapshot>` (a single global `last_updated_ms`), with exponential backoff and a proactive 15-min reconnect to dodge Polymarket's zombie-connection bug.
- **MmActor** (`mm.rs`, 920): consumes the sybil SSE block stream (hand-rolled parser), runs a four-sided Avellaneda-Stoikov quote engine with exposure-decayed budget and in-batch NegRisk complete-set filtering, submits one IOC batch per block capped at 64 orders, and posts reference prices.
- **ResolutionActor** (`resolution.rs`): optional (needs `SIGNER_KEY_PATH`, **set in neither compose file** — so auto-resolution is disabled in all deployments); polls closed events and signs P256 attestations for clean binary settlements.

Float→nano conversion happens once at order construction on range-clamped values; the all-integer core convention is respected in spirit. The mapping `MappingStore` lives behind an `Arc<RwLock>` shared across actors and is **also mounted read-only into the arena container** and parsed directly by `arena/live/news_feed.py`.

## Strengths

- Clean four-actor decomposition matching the tokio-actor convention; `select!` panic→shutdown; explicit no-op placeholder for the disabled resolution actor.
- Consistent pure-function extraction with real unit tests (`generate_quotes`, `build_metadata_request`, `decode_midpoints_response`, ISO-8601 parser).
- Deliberate schema-drift tolerance at the external boundary (`#[serde(flatten)]` catch-all, `string_or_float` shim, negRisk aliases, unknown-message tolerance).
- Operationally literate comments (the zombie-reconnect workaround, `/midpoints` chunking, order-cap matched to the API's DOS guard).

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| PM-1 | bug | high | Resolved/untradeable market poisons the **entire** MM batch: the MM never untracks markets, and the sequencer rejects the whole submission if any order targets a non-tradeable market → mirror stops providing liquidity across many markets, forever |
| PM-2 | bug | high | NegRisk incremental re-sync creates **duplicate overlapping** `MarketGroup`s in canonical state (the sequencer pushes with no dedup), double-counting coverage in complete-set checks |
| PM-3 | bug | high | Resolution reconciler scans a top-50-closed-events window with no category filter; a mirrored market that closes can be permanently missed → positions never pay out |
| PM-4 | bug | medium | Price staleness is **global**, not per-token: as long as any token updates, a frozen token's stale midpoint is quoted indefinitely (up to the 15-min reconnect) → picked off |
| PM-5 | bug | medium | Hand-rolled SSE parser drops blocks: no cross-chunk buffering, only the first event per chunk consumed; multi-byte UTF-8 split across chunks is corrupted |
| PM-6 | bug | medium | Reference prices never expire/evict; a market drifting out of band keeps its last in-band value forever, and the arena trades on it (`--require-reference-prices`) |
| PM-7 | design | medium | Fresh MM account created every process start ($1M minted per restart); previous inventory is orphaned while real exposure persists |
| PM-8 | bug | medium | `live_market_count` only grows; after weeks, all 200 slots are consumed by dead markets and fresh events are never quoted |
| PM-9 | bloat | medium | Dead code and dead config: `rest_poll_interval_secs` documented but never read; several unused client methods, error variants, and parsed-but-unread fields; `lib.rs`-re-export layout suppresses dead-code lints |
| PM-10 | design | medium | Mapping JSON is a leaky cross-container API written non-atomically; the arena parses its private serialization and never refreshes it → markets mirrored after arena start have no prices |
| PM-11 | design | medium | Whole pipeline structurally depends on dev-mode endpoints; prod runs `SYBIL_DEV_MODE=true` because the mirror needs them — see [Theme 10](02-cross-cutting-themes.md), [18](18-ops-deployment.md) |
| PM-12 | ops | medium | Resolution actor + feed pubkey configured nowhere → auto-resolution silently off in all deployments; every mirrored market needs manual admin resolution |
| PM-13 | doc-drift | medium | Crate `AGENTS.md` badly stale: "three actors" (four), lists a nonexistent module, contradicts the now-intended shared-DTO design |
| PM-14 | test-gap | medium | `sync.rs` (the most intricate logic, where PM-1/PM-2 live) and the SSE/WS transport have zero tests |
| PM-15 | inconsistency | low | Shared `Arc<RwLock<MappingStore>>` and a pointless `Mutex` inside a single-task actor violate the actor convention |

## Ambitious ideas

1. **Make the MM lifecycle-aware via data it already receives:** consume `MarketResolved` from the block stream to untrack markets, and add a small watch-channel live-set protocol back to Sync so the `mm_max_markets` budget self-heals. This dissolves PM-1, PM-4, and PM-8 in one structural move.
2. **Turn SyncActor into a reconciliation loop over a pure diff:** fetch Gamma state, snapshot the mapping, compute `Vec<SyncAction>` (CreateMarket, UpsertGroup, PushMetadata, AdmitToMm, FlagClosed) in a pure, unit-testable planner, then execute. Group upsert-by-event-id kills PM-2; per-condition resolution reconciliation (merging the ResolutionActor into the same loop) kills PM-3.
3. **Delete the mapping-file-as-API:** `sybil-api` already stores `polymarket_condition_id` per market; add CLOB token ids to that metadata and have the arena consume the API instead of parsing the mirror's private JSON across a Docker volume. The `MappingStore` becomes a purely internal, actor-owned cache.
4. **Replace the dev-mode dependency with a first-class service identity:** the crate already owns a P256 signer — extend signed submission to cover account creation/metadata/reference prices so `SYBIL_DEV_MODE` can be off in prod.
5. **Restructure around a per-token price fabric** (`token → (price, updated_at_ms, source)`) with per-token staleness and eviction on market close, resurrecting the dead periodic REST reconciliation. The whole-map clone-per-update also disappears.
6. **Split `mm.rs`:** the quote engine into a tested `quoting` module, leaving `MmActor` a thin I/O shell — matching the crate's own pure-core/imperative-shell style.
