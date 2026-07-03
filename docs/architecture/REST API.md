---
tags: [infrastructure, crate]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-03
---

The REST API is the external interface to the exchange. Built with Axum (a Rust async web framework), it exposes endpoints for account management, market operations, order submission, and block retrieval. An OpenAPI schema is auto-generated for client code generation. The API communicates with the sequencer via message passing through a `SequencerHandle` — no shared mutable state.

The endpoint groups are: **System** (`/v1/health`, `/v1/state-root`), **Proofs** (`/v1/proofs/state/{leaf_key_hex}`), **Accounts** (create, query balance/positions, fund, register keys), **Markets** (list, create, query details/prices/groups, resolve), **Orders** (submit unsigned or [[P256 Authentication|signed]]), **Bridge** (status, account bridge keys, L1 deposits, signed/unsigned withdrawal leaves), and **Blocks** (latest, by height, [[SSE Block Stream|SSE stream]]). Operator/service writes and bridge operations (account creation/funding, market creation/grouping/resolution, mirror metadata and reference prices, raw event snapshots, feed registration, bridge reverse-key lookup, L1 deposit ingestion, and withdrawal creation) are mounted in production but require `Authorization: Bearer $SYBIL_SERVICE_TOKEN`; an unset token fails closed. Dev mode skips that service bearer check for local workflows and additionally mounts only simulation pause/resume plus diagnostic all-pending/orderbook listings.

Bridge deposit ingestion is scaffolding for [[L1 Settlement and Vault]] rather than a completed trust boundary. `POST /v1/bridge/deposits` is service-only and credits the sequencer through the existing `pending_l1_deposits` WAL, but today it trusts the operator/indexer-supplied L1 event fields. `POST /v1/bridge/withdrawals/signed` verifies a P256 signature over the canonical withdrawal payload against the account key registry before using the existing `pending_bridge_withdrawals` WAL; `POST /v1/bridge/withdrawals` remains a service-only operator path. SYB-178/SYB-188 still need proof-backed L1 deposit inclusion/finality and vault withdrawal authorization before these paths are production trust-complete.

Order quantity fields (`quantity`, `max_fill`, `fill_qty`,
`remaining_quantity`, `original_quantity`, and position `quantity`) are protocol
[[Fractional Quantities|share-units]], not display shares. `1000` units equals
1 full YES/NO share; the minimum increment is `1` unit = 0.001 share. Client
layers may expose ordinary decimal shares, but signed/canonical API payloads
use integer units.

Market raw price history is served through
`GET /v1/markets/{id}/prices/history`, backed by durable `price_points` when a
store is configured. The endpoint is bounded by `limit` and pages older raw
points with `before_height` / `next_before_height`. When raw price retention is
active, responses include `retention_min_height` so clients can distinguish an
empty in-retention range from data older than retained history.

Long-window charting uses `GET /v1/markets/{id}/prices/candles`, backed by
durable post-seal price candles. Candles are built only from committed batch
price points: open/high/low/close are over sealed YES/NO prices, volume is
post-seal traded notional, and empty buckets are omitted. This preserves the
private-batch boundary because no in-flight order-book information is exposed.

Account fill history is served by `GET /v1/accounts/{id}/fills`. New clients
tail with `after=<cursor>&limit=N`, where each response row includes an opaque
stable `cursor` string (`0.0` starts at the beginning) and results are returned
oldest-to-newest. The older `offset` query remains compatibility-only and pages
newest-first.

Block history reads distinguish missing data from retained-history gaps:
`GET /v1/blocks/{height}` returns `410 Gone` with code `RETENTION_GONE` when
the requested height is below the retained `blocks_full` floor. WebSocket block
replay sends a versioned `retention_gap` envelope before closing when
`?from_block=` starts before durable block retention.

The API is stateless — all exchange state lives in the `SequencerActor`. Order-write endpoints first pass through a cheap HTTP token bucket keyed globally and by client address/header. This happens before JSON parsing and P256 signature verification, so malformed or invalid signed traffic cannot consume unbounded CPU. When an order submission arrives at `POST /v1/orders`, the API handler converts the [[Order Types|OrderSpec]] to the engine's internal representation and sends it to the sequencer via a channel. The sequencer either directly admits simple single-market orders into the resting book or defers MM / bundle / multi-market submissions via the [[Mempool|deferred-submission buffer]]. When a client queries `GET /v1/blocks/latest`, the API reads the latest sealed block from the sequencer's state, including both `state_root` and `events_root`. A sealed block is a canonical `Block` plus the derived [[Block Data Boundaries|`BlockAnalytics` sidecar]]; the API joins them into one client response, but verifier/prover paths should consume canonical block and witness data, not product analytics. This actor-model architecture (inspired by Tokio actor patterns) means the API layer can be horizontally scaled without coordination — all instances talk to the same sequencer actor.

The sequencer message path is monitored by [[Actor Mailbox Monitoring]]. The API exports `sybil_actor_queue_depth{actor="sequencer"}` from `/metrics`, with configurable warning and critical thresholds, so operator alerts distinguish actor backlog from solver latency or HTTP rejection pressure.

`GET /v1/proofs/state/{leaf_key_hex}` serves the current committed typed-state
qMDB proof. The path parameter is the hex-encoded canonical state key (for
example `acct/{account_id_be_u64}`). Responses are anchored to the latest
persisted block height and include either a qMDB inclusion proof with the
canonical leaf value or a qMDB exclusion proof for absent keys. This endpoint
requires a persistent store-backed sequencer; in-memory dev sequencers return
`503 Service Unavailable`.

## Key Properties
- Axum-based, async, with OpenAPI auto-generation
- Actor model: API → message channel → `SequencerActor`
- All values in [[Nanos and Integer Arithmetic|nanos]] (u64)
- Service bearer auth gates operator writes in production; dev mode is limited to local conveniences and diagnostics
- Order-write endpoints return `429 Too Many Requests` with `Retry-After` under abnormal load
- State proof endpoint returns inclusion or exclusion proofs for the latest committed qMDB root
- Order submissions support GTC, IOC, and GTD time-in-force
- Stateless API layer — all state in sequencer
- CORS is permissive only in dev mode; production uses `SYBIL_CORS_ORIGINS` and defaults to same-origin only

## Where This Lives
> `crates/sybil-api/src/app.rs` — router creation, OpenAPI schema
> `crates/sybil-api/src/routes/` — endpoint handlers
> `crates/sybil-api/src/state.rs` — `AppState` with `SequencerHandle`

## See Also
- [[Order Types]] — the `OrderSpec` enum submitted via the API
- [[SSE Block Stream]] — real-time block push via `/v1/blocks/stream`
- [[WebSocket Block Stream]] — production block stream at `/v1/blocks/ws` with replay + backpressure
- [[Block Data Boundaries]] — API composition vs. canonical protocol data
- [[P256 Authentication]] — signed order submission
- [[Actor Mailbox Monitoring]] — sequencer queue-depth metric and alerts
- [[Block Lifecycle]] — what happens after an order enters the system
- [[State Root Schema]] — canonical typed-state key/value commitment
