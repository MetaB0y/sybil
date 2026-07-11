# HTTP API Layer

**Crates:** `sybil-api`, `sybil-api-types`, `sybil-signing`

## Verdict

A clean Axum layer with a genuinely excellent router trust-boundary design, undermined by three things: no replay protection on signed writes, a half-done read-model migration that storms the sequencer actor on every dashboard poll, and an `AppState` god-struct that quietly contradicts the "all state lives in the sequencer" claim. The write path respects the actor boundary; the read path does not.

## Architecture as built

`sybil-api` (~6k LOC) parses `ApiConfig` (clap, 26 fields), opens the redb store, spawns the sequencer via `SequencerHandle`, and serves `create_router`. **Router trust split (the standout):** ui/ops/metrics/public routes are always mounted; `dev_api_routes`/`internal_dev_routes` are mounted *only* when `dev_mode`, so disabling dev mode physically unmounts write routes at the router boundary, and route-policy tests assert the exact mount table.

**Write path:** all mutations are RPC calls to the sequencer actor (`SequencerMsg`); no shared mutable protocol state. **Read path:** only three cold routes (account fills/events/equity) use `read_model_store()` (redb directly, via `spawn_blocking`); every other read (`list_markets`, `get_market`, search, prices, portfolio, blocks) enqueues actor messages — `list_markets` fires a 10-way `try_join` of RPCs per request.

**Streaming:** both SSE (`sse.rs`, 26 LOC) and WebSocket (`ws.rs`, 216 LOC) sit on the same broadcast channel; WS has a versioned envelope, replay, lag signalling, ping/idle-timeout — SSE has none of these.

**Auth:** P256 ECDSA. Signed writes parse hex pubkey+sig in the route, build a `SignedOrder`/`SignedCancel`/`SignedBridgeWithdrawal`, and the actor verifies against the registered key via `sybil-signing` borsh bytes. The signature covers order content including `expires_at_block` but **no nonce/sequence**.

**Off-block state:** `AppState` also holds `reference_prices`, `market_ref_data` (with JSON-on-disk persistence), an event-snapshot directory, an arena SQLite path, and the HTTP rate limiter — a parallel state store outside the sequencer, mutated by dev/internal routes. This contradicts `REST API.md`'s "the API is stateless — all exchange state lives in the SequencerActor."

**`sybil-signing` (194 LOC)** holds a *third* copy of `Order`/`MarketId`/`PriceCondition`/etc. for stable borsh signable bytes, deliberately mirroring the sequencer/oracle structs "without importing" them.

## Strengths

- **Router trust-boundary design is excellent** and test-asserted: a new write route cannot silently become public.
- **Error handling is centralized and consistent:** a single `AppError` with one `From<SequencerError>` mapping every variant to a correct HTTP status + stable machine code, plus `Retry-After` on 429.
- `sybil-api-types` as a shared DTO crate is the right call — server and `sybil-polymarket` share one source of truth, with an `openapi` feature gate keeping utoipa out of the client build.
- The pre-parse HTTP token-bucket runs before JSON parse and P256 verification — the ordering deliberately bounds CPU from malformed signed traffic.
- The WebSocket stream is carefully engineered (subscribe-before-fetch-head, replay dedupe, versioned envelope, explicit lag signalling) and integration-tested.
- Canonical signing bytes are isolated with insta snapshots; `crypto.rs` has focused tamper/expiry/id-exclusion tests.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [D3](01-critical-bugs.md) | design | high | Signed orders/cancels have **no replay/nonce protection**; re-POSTing a signed payload to the public endpoint creates a new resting order every time |
| [D4](01-critical-bugs.md) | bug | high | Unbounded order quantity overflows i64 balance validation (`validation.rs:39`); reachable from public `POST /v1/orders/signed` |
| API-1 | design | medium | Read-model migration half-done: `list_markets` fires 10 RPCs/poll, `get_block_by_height` only scans the 100-entry ring (404s persisted blocks) — see [Theme 4](02-cross-cutting-themes.md) |
| API-2 | inconsistency | medium | `0x`-prefix handling diverges: `bridge.rs`/`proofs.rs` strip it, `orders.rs`/`accounts.rs`/`feeds.rs` don't — same value works on one signed endpoint and 400s on another |
| API-3 | design | medium | `AppState` is a god-struct mixing the sequencer handle with a parallel off-block store (reference prices, market ref data, event snapshots) — contradicts the "stateless API" doc |
| API-4 | bloat | medium | Two identical `TokenBucket` implementations (`state.rs` and `admission.rs`) |
| API-5 | bloat | medium | `markets.rs` is a 901-line god-file with a 20-field response-builder struct rebuilt three times; `now_ms` duplicated within the file |
| API-6 | debt | medium | Three parallel `Order` representations hand-mapped; a new `Order` field is silently omitted from signed bytes unless `to_canonical_order` is updated — see [Theme 6](02-cross-cutting-themes.md) |
| CM/WK | bug | medium | `u64→i64` wrap in dev-mode account create/fund (`accounts.rs:49,75`) allows negative balances on the public devnet (dev-mode is on in prod) |
| API-7 | inconsistency | low | `get_account_fills` has no upper `limit` cap while `get_account_history` caps at 500 — a client can force the store to accumulate the entire fill history |
| API-8 | ops | low | `/metrics` scrape triggers four hot-actor RPCs per Prometheus poll, coupling the metrics path to the mailbox pressure it observes |
| API-9 | inconsistency | low | SSE duplicates WebSocket but lacks versioning/replay/backpressure and silently drops lagged blocks — see [Theme 7](02-cross-cutting-themes.md) |
| API-10 | bloat | low | Duplicated `now_ms` / P256-parse helpers across route modules |
| API-11 | inconsistency | low | Dev handlers keep redundant internal `dev_mode` checks that are unreachable when the route is unmounted (dead 403 branch) |

## Ambitious ideas

1. **Finish the hot/cold read-model split** — the single highest-leverage move for "crispy clear." A typed `ReadModelStore` covering markets, blocks-by-height, price history, and platform aggregates, with all cold reads routed there via `spawn_blocking`. The actor then serves only writes + latest-state, and `list_markets` stops issuing 10 RPCs per poll.
2. **Collapse the three `Order` representations.** Make `sybil-signing` the one canonical signable form and have `matching_engine::Order` implement `To/FromCanonical` (feature-gated) so signed bytes are *derived*, not hand-mapped, with a field-exhaustiveness test so a new field cannot escape the signature.
3. **Extract off-block display state** (reference prices, market ref data, event snapshots) into a dedicated `RefDataService` (its own actor or store). `AppState` shrinks to `{sequencer, read_store, config, rate_limiter}`, restoring the "all protocol state in the sequencer" invariant behind one clearly-labelled display sidecar.
4. **Add signed-write replay protection** as a first-class concept: a nonce/expiry in the canonical payload plus a bounded per-account replay set in the sequencer, shared by orders, cancels, and withdrawals — a prerequisite before any signed endpoint goes public.
5. **Unify the block feed on WebSocket** (arena and polymarket already consume it) and delete SSE, or make SSE a thin re-encoding of the same `BlockStreamPayload`. One feed, one guarantee set.
6. **Fix `u64`/Nanos serialization once in `sybil-api-types`** (`serde_with::DisplayFromStr`, `format:int64` strings) so the >2^53 JSON-number corruption disappears for every client and the frontend's `patch-bigints.mjs` can be deleted (see [17-frontend](17-frontend.md)). Add `[profile.release] overflow-checks = true` while auditing the `as i64` multiplies.
