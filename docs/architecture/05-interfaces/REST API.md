---
tags: [infrastructure, crate]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-13
---

The REST API is the external interface to the exchange. Built with Axum, it
exposes account management, market operations, order submission, block
retrieval, and historical product views. OpenAPI is generated for clients.
Current exchange reads/writes use `SequencerHandle`; historical reads are
owner-authorized here and proxied to the private `sybil-history` service.

The endpoint groups are: **System** (`/v1/health`, `/v1/state-root`), **Proofs** (`/v1/proofs/state/{leaf_key_hex}`), **Data Availability** (`/v1/da/{height}/manifest`, `/v1/da/{height}/payload`), **Accounts** (create, query balance/positions, fund, register keys), **Markets** (list, create, query details/prices/groups, resolve), **Orders** (submit unsigned or [[P256 Authentication|signed]]), **Bridge** (status, account bridge keys, L1 deposits, signed/unsigned withdrawal leaves), and **Blocks** (latest, by height, privacy-preserving [[WebSocket Block Stream|public WebSocket stream]] at `/v2/blocks/ws?from_block=N`, plus [[SSE Block Stream|SSE]] as a third-party convenience). `/v1/health` reads committed height and genesis hash in one actor snapshot; snapshot failure returns 503 rather than reporting a partial chain identity as healthy. Operator/service writes, the state-proof and DA-payload custody surfaces, authenticated canonical v1 block stream, and bridge operations require `Authorization: Bearer $SYBIL_SERVICE_TOKEN`; an unset token fails closed. Dev mode skips that service bearer check for local workflows and additionally mounts simulation pause/resume, diagnostic all-pending/orderbook listings, and the explicit unverified [[Attestation|attestation shape stub]].

Per-account reads (`/accounts/{id}`, portfolio, fills, equity, events, orders,
signing-key metadata, read-key metadata, bridge key, active withdrawals, and private summary) require
either an active read-scoped bearer owned by `{id}` or the service token. A
wrong-account read bearer is `403`; missing, invalid, or revoked read credentials
are `401`. Public market, activity, aggregate-statistics, and leaderboard reads
remain unauthenticated. Leaderboard rows are limited to accounts that
explicitly opt in by signing a non-empty public display name; the settings UI
discloses the financial fields that choice publishes.

Public block REST/SSE responses are an allowlisted market tape: header
commitments, prices, aggregate analytics, the bridge deposit root/count, and
sanitized resolved-market ids. They do not contain fills, rejection rows,
account-bearing system events, individual bridge leaves, or derived order
lifecycle rows. The old full v1 WebSocket response is service-authenticated;
public replay uses the distinct v2 endpoint and DTO. See
[ADR-0016](../../adr/0016-public-market-tape-and-recovery-da-boundaries.md).

`GET /v1/accounts/{id}/keyop-state` is intentionally public and exposes only
the current committed `keys_digest` and `events_digest`. Clients fetch it just
before signing key registration/revocation; it contains no key list, balance,
position, or profile data. Admission rejects a stale state binding with 409.

Public account onboarding uses `POST /v1/accounts` with both
`initial_balance_nanos` and `initial_key`. The API sends both through one
sequencer actor command and one control-plane WAL row, so restart cannot expose
an acknowledged onboarding account without its initial key. Omitting
`initial_key` retains the deprecated bare-account form for service/dev tooling
only; the old unsigned `POST /accounts/{id}/keys` bootstrap is likewise
service-only.

> [!warning] Public onboarding is not production-safe yet
> The route currently permits unlimited free accounts and caller-selected demo
> balances. Read-API-key recovery state and durable resting-order admission are
> now bounded, but free account creation and the history outbox during a
> prolonged projector outage still lack stock or byte budgets. Order token
> buckets limit flow, not all
> accumulated state. See the
> [2026-07-11 resource audit](https://github.com/MetaB0y/sybil/blob/main/design/dos-audit-2026-07-11.md).

Bridge deposit ingestion is scaffolding for [[L1 Settlement and Vault]] rather than a completed trust boundary. `POST /v1/bridge/deposits` is service-only and credits the sequencer through the existing `pending_l1_deposits` WAL, but today it trusts the operator/indexer-supplied L1 event fields. `POST /v1/bridge/withdrawals/signed` verifies a P256 signature over the canonical withdrawal payload against the account key registry before using the existing `pending_bridge_withdrawals` WAL; `POST /v1/bridge/withdrawals` remains a service-only operator path. SYB-178/SYB-188 still need proof-backed L1 deposit inclusion/finality and vault withdrawal authorization before these paths are production trust-complete.

The service-gated indexer advances `POST /v1/bridge/l1-height` after each fully
processed confirmed scan range. That existing scan cursor is the withdrawal
expiry clock. Crossing `expiry_height` emits a refund event, restores the
account balance exactly once, and retires the leaf in the same committed block.
`refunded` is exposed as a withdrawal status while the terminal event is
pending; replays after pruning are accepted as no-ops.

`GET /v1/accounts/{id}/withdrawals` is the only account withdrawal-detail read
and is owner-scoped. The formerly enumerable
`GET /v1/bridge/withdrawals/{id}` route is removed.
It returns active leaves, including a terminal status during the short interval
before the next block retires that leaf. Historical block responses preserve
the immutable creation-time leaf and are not a current withdrawal-status API.

Order quantity fields (`quantity`, `max_fill`, `fill_qty`,
`remaining_quantity`, `original_quantity`, and position `quantity`) are protocol
[[Fractional Quantities|share-units]], not display shares. `1000` units equals
1 full YES/NO share; the minimum increment is `1` unit = 0.001 share. Client
layers may expose ordinary decimal shares, but signed/canonical API payloads
use integer units.

Market raw price history is served through
`GET /v1/markets/{id}/prices/history`, backed by the private history projector.
The endpoint is bounded by `limit` and pages older committed points with
`before_height` / `next_before_height`.

Long-window charting uses `GET /v1/markets/{id}/prices/candles`. The history
service builds candles only from committed batch price facts:
open/high/low/close are over sealed YES/NO prices, volume is post-seal traded
notional, and empty buckets are omitted. No in-flight order-book information is
exposed.

Account fill history is served by `GET /v1/accounts/{id}/fills`. New clients
tail with `after=<cursor>&limit=N`. The response envelope includes the
compatibility retention fields plus `indexed_through_height` and
`history_complete_from_height`; forward rows are oldest-to-newest and each has
a stable cursor. The older `offset` query remains compatibility-only and pages
newest-first.

`GET /v1/accounts/{id}/events` uses the analogous `events`/`next_before`
envelope. Equity responses carry the same projector checkpoint/completeness
fields. `all` means all rows available to the current projector. History
service failures return `503 HISTORY_UNAVAILABLE`; they are never replaced by
an empty in-memory response. Equity is bounded to 5,000 represented points and
reports `source_points` plus `downsampled`.

All account history remains owner-scoped at the public API. Internal history
routes and raw account-attributed batches use a dedicated
`SYBIL_HISTORY_TOKEN`, are private service surfaces, and are not browser
endpoints. Public market history uses the same projector but only committed
market facts. See [[Historical Data Serving]].

Block history reads distinguish missing data from retained-history gaps:
`GET /v1/blocks/{height}` returns `410 Gone` with code `RETENTION_GONE` when
the requested height is below the retained `blocks_full` floor. First-party
WebSocket block replay sends a versioned `retention_gap` envelope before
closing when `?from_block=` starts before durable block retention. SSE remains
a live-only convenience stream and does not expose a replay cursor or retained
floor.

All exchange mutation remains in the `SequencerActor`. Order-write endpoints
first pass through cheap global/per-client HTTP token buckets before JSON or
P256 work. Handlers convert public DTOs to domain instructions and send them to
the actor, which directly admits ordinary supported orders or defers atomic
bundles/MM submissions. Current block/account reads also come from actor
snapshots. Historical range queries take the independent private service path
and therefore cannot occupy the sequencer actor or scan its database.

The sequencer message path is monitored by [[Actor Mailbox Monitoring]]. The API exports `sybil_actor_queue_depth{actor="sequencer"}` from `/metrics`, with configurable warning and critical thresholds, so operator alerts distinguish actor backlog from solver latency or HTTP rejection pressure.

`GET /v1/proofs/state/{leaf_key_hex}` serves the current committed typed-state
qMDB proof. The path parameter is the hex-encoded canonical state key (for
example `acct/{account_id_be_u64}`). Responses are anchored to the latest
persisted block height and include either a qMDB inclusion proof with the
canonical leaf value or a qMDB exclusion proof for absent keys. This endpoint
requires a persistent store-backed sequencer; in-memory dev sequencers return
`503 Service Unavailable`. Its range-proof payload follows Commonware 2026.5:
`leaves`, `inactive_peaks`, proof digests, optional partial-chunk digest, and
`ops_root`.

`GET /v1/da/{height}/manifest` and `GET /v1/da/{height}/payload` expose the
retained canonical witness payload and its typed DA manifest. The manifest is
public; the payload requires the service token. They require store-backed
DA artifact rows written after block commit; the small manifest is cached in a
separate row in the same transaction as the payload artifact. In-memory
sequencers and artifact gaps return `404 Not Found`, including the oldest
retained block-history height when the store knows it. Manifest reads therefore
do not load or hash the witness payload. Payload reads still recompute
`payload_root` over the returned bytes and fail closed with `500` on a mismatch.
Clients must still verify the SYB-80 binding chain themselves:
`payload_root -> witness_root -> da_commitment -> L1 RootRecord`. Retention is
the existing block-history retention behavior, not a new DA policy.
Both retained DA routes share a dedicated global/per-client token bucket and a
hard in-flight concurrency cap before dispatching store work.

## Key Properties
- Axum-based, async, with OpenAPI auto-generation
- Actor model for current state/mutation; private HTTP projection for history
- All values in [[Nanos and Integer Arithmetic|nanos]] (u64)
- Service bearer auth gates operator writes in production; dev mode is limited to local conveniences and diagnostics
- Order-write endpoints return `429 Too Many Requests` with `Retry-After` under abnormal load
- Retained DA reads return `429` when their dedicated rate or concurrency budget is exhausted
- State proof endpoint returns inclusion or exclusion proofs for the latest committed qMDB root
- DA endpoints expose public typed manifests and service-gated retained witness payload bytes
- Public block endpoints expose a typed market-tape projection; canonical account rows are service-only
- Order submissions support GTC, IOC, and GTD time-in-force
- Stateless API layer — all state in sequencer
- CORS is permissive only in dev mode; production uses `SYBIL_CORS_ORIGINS` and defaults to same-origin only
- `GET /v1/attestation` is an unverified shape stub mounted only in dev mode; production returns 404 until real Nitro verification exists

## Where This Lives
> `crates/sybil-api/src/app.rs` — router creation, OpenAPI schema
> `crates/sybil-api/src/routes/` — endpoint handlers
> `crates/sybil-api/src/state.rs` — `AppState` with `SequencerHandle`

## See Also
- [[Order Types]] — the `OrderSpec` enum submitted via the API
- [[WebSocket Block Stream]] — public v2 market tape and authenticated v1 canonical stream
- [[SSE Block Stream]] — third-party convenience stream via `/v1/blocks/stream`
- [[Block Data Boundaries]] — API composition vs. canonical protocol data
- [[P256 Authentication]] — signed order submission
- [[Actor Mailbox Monitoring]] — sequencer queue-depth metric and alerts
- [[Attestation]] — dev-only enclave-attestation shape and the real-verification boundary
- [[Block Lifecycle]] — what happens after an order enters the system
- [[State Root Schema]] — canonical typed-state key/value commitment
