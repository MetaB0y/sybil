---
tags: [infrastructure, crate]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-17
---

The REST API is the external interface to the exchange. Built with Axum, it
exposes account management, market operations, order submission, block
retrieval, and historical product views. OpenAPI is generated for clients.
Current exchange reads/writes use `SequencerHandle`; historical reads are
owner-authorized here and proxied to the private `sybil-history` service.

## Units

Protocol quantity fields use integer share-units (`1000` units = 1 share).
Money and every `*_nanos` field use integer nanodollars
(`1_000_000_000` = $1); price and payout nanos are per-share probabilities in
`[0, 1e9]`.

All `*_nanos` values cross REST and WebSocket JSON as exact base-10 strings,
including values nested in `clearing_prices_nanos` arrays. Rust DTOs retain
`u64`/`i64` internally. The API accepts legacy integer tokens on input during
migration but always emits strings, and OpenAPI advertises the string contract.
Clients must parse these values with integer/big-integer arithmetic rather than
floating point.

The endpoint groups are: **System** (`/v1/health`, `/v1/state-root`), **Proofs** (`/v1/proofs/state/{leaf_key_hex}`), **Data Availability** (`/v1/da/{height}/manifest`, `/v1/da/{height}/payload`), **Accounts** (create, query balance/positions, fund, register keys), **Markets** (list, create, query details/prices/groups, resolve), **Orders** (submit unsigned or [[P256 Authentication|signed]]), **Bridge** (status, account bridge keys, L1 deposits, signed/unsigned withdrawal leaves), and **Blocks** (latest, by height, and the privacy-preserving [[WebSocket Block Stream|public WebSocket stream]] at `/v2/blocks/ws?from_block=N`). `/v1/health` reads committed height and genesis hash in one actor snapshot; snapshot failure returns 503 rather than reporting a partial chain identity as healthy. Operator/service writes, the state-proof and DA-payload custody surfaces, authenticated canonical v1 block stream, and bridge operations require `Authorization: Bearer $SYBIL_SERVICE_TOKEN`; an unset token fails closed. Dev mode skips that service bearer check for local workflows and additionally mounts simulation pause/resume, diagnostic all-pending/orderbook listings, and the explicit unverified [[Attestation|attestation shape stub]].

When `SYBIL_EVENT_SNAPSHOT_DIR` is configured, startup requires that directory
to be usable. Raw event PUTs publish through a unique same-directory temporary
file, sync its contents, atomically rename it, and sync the parent directory on
Unix. Readers therefore see a complete old or new snapshot across restart;
configured persistence does not silently degrade to volatile behavior.

Per-account reads (`/accounts/{id}`, portfolio, fills, equity, events, orders,
signing-key metadata, read-key metadata, bridge key, active withdrawals, and private summary) require
either an active read-scoped bearer owned by `{id}` or the service token. A
wrong-account read bearer is `403`; missing, invalid, or revoked read credentials
are `401`. Public market, activity, aggregate-statistics, and leaderboard reads
remain unauthenticated. Leaderboard rows are limited to accounts that
explicitly opt in by signing a non-empty public display name; the settings UI
discloses the financial fields that choice publishes.

Public block REST/WebSocket responses are an allowlisted market tape: header
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

Public self-service onboarding is a separate surface:
`GET /v1/onboarding` reports the fixed grant and remaining lifetime account
stock, while `POST /v1/onboarding/accounts` accepts only `initial_key`. The
server chooses `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS`; unknown fields are rejected,
so an anonymous caller cannot select its funding. A dedicated global/client
token bucket limits request flow before key parsing and actor work.

`SYBIL_PUBLIC_ACCOUNT_CAPACITY` bounds the total account-id stock visible to
anonymous onboarding. Allocation reads the actor's durable next account id and
holds the bootstrap lock through the one-command account/key write, so parallel
callers cannot overshoot and restart cannot expose an acknowledged account
without its initial key. Account ids, key history, and witness references are
permanent protocol identities: v1 does not delete, tombstone, recycle, or
reclaim them. The cap is therefore deliberately a lifetime stock limit, not a
concurrent-user limit. Exhaustion returns stable error code
`PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED` with HTTP 409.

Explicitly funded `POST /v1/accounts` is service/dev-only. This trusted operator
surface is intentionally outside the anonymous cap for bots, fixtures, and
recovery operations; its use is observable in total sequencer account stock and
also consumes ids, so it can reduce the remaining public allocation. The old
unsigned `POST /accounts/{id}/keys` bootstrap is likewise service-only.

An L1 deposit does not allocate an account. A deposit for an unknown Sybil key
is capital-backed by the token transfer and L1 gas but remains in the bounded
quarantine workflow until that key is registered on an account allocated by
one of the normal routes; quarantine cannot bypass the account-stock ceiling.
The protocol currently imposes no additional economic minimum above a valid
positive token-unit deposit. The `private-devnet` posture requires a nonzero
fixed play-money grant while retaining the locked persistence/authentication
guardrails. The `prod` posture fixes that grant at zero and fail-closes a
nonzero override. Any vault minimum/state-rent policy must be ratified together
with its refund, quarantine, and rounding semantics rather than invented as an
API-only minimum.

> [!warning] Other stock budgets remain open
> Public account stock, read-API-key recovery state, and durable resting-order
> admission are bounded, but the product-history outbox during a prolonged
> projector outage still lacks a ratified byte/row overflow policy. Exact
> logical bytes, rows, oldest/newest height, oldest age, and host disk capacity
> are monitored for devnet; no row is silently dropped. See the
> [2026-07-11 resource audit](https://github.com/MetaB0y/sybil/blob/main/design/dos-audit-2026-07-11.md)
> and [GitHub #90](https://github.com/MetaB0y/sybil/issues/90).

Bridge deposit ingestion is service-only. The indexer authenticates the lowest
common finalized prefix of its configured provider set, requires unanimous
block hashes, and binds both log and state reads to exact canonical block
hashes before submitting `POST /v1/bridge/deposits`. The sequencer then commits
the `L1Deposit` through the globally ordered [[Acknowledged-Write WAL
Replay|acknowledged-write log]] and reconstructs the deposit root from every
leaf field and its committed frontier; the eventual transition checkpoint is
also matched against the real vault by `SybilSettlement`. See [[L1 Settlement
and Vault]] for the provider-quorum and fail-stop recovery boundary.
`POST /v1/bridge/withdrawals/signed` verifies a P256 signature over the
canonical withdrawal payload against the account key registry before appending
the corresponding `BridgeWithdrawal` write; `POST /v1/bridge/withdrawals`
remains a service-only operator path. Both monetary bridge creation routes and
deposit ingestion first require one all-or-none API domain configured by
`SYBIL_BRIDGE_CHAIN_ID`, `SYBIL_BRIDGE_VAULT_ADDRESS`, and
`SYBIL_BRIDGE_TOKEN_ADDRESS`. Absence returns `503 BRIDGE_UNAVAILABLE`; a
different request domain returns `400 BRIDGE_DOMAIN_MISMATCH` before sequencer
mutation. Public `GET /v1/bridge/status` exposes the configured domain or its
absence so operators and clients do not infer bridge readiness from route
existence.

`GET /v1/bridge/withdrawals/pending` is the service-authenticated operator
relay feed. It returns only active `not_requested` leaves, ordered by withdrawal
id. It is intentionally separate from owner-scoped account status and from
public block data: recipients, amounts, and account attribution are private.
The unsafe Sepolia relay consumes this feed, while the confirmed-log indexer is
still the only component allowed to advance queued/finalized/cancelled status.

The service-gated indexer advances `POST /v1/bridge/l1-height` after each fully
processed confirmed scan range. That existing scan cursor is the withdrawal
expiry clock. Crossing `expiry_height` emits a refund event, restores the
account balance exactly once, and retires the leaf in the same committed block.
`refunded` is exposed as a withdrawal status while the terminal event is
pending; replays after pruning are accepted as no-ops.

The indexer now persists the canonical hash of the last processed L1 block
beside that cursor and validates it before making any later API call. Deposit
and withdrawal logs are also bound to canonical block hashes. A mismatch is
durably fail-stop latched in the cursor; the API has no automatic inverse for
events already applied, so operators must freeze trading/contracts and follow
the [L1 reorg recovery runbook](../../runbooks/l1-reorg-recovery.md).

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

Atomic order submission reports market lifecycle rejection through the shared
error envelope, not through prose conventions. An unknown market returns `404
MARKET_NOT_FOUND` with `details.market_id`; a market that exists but is no
longer tradeable returns `409 MARKET_NOT_TRADEABLE` with `details.market_id`
and `details.market_status`. Bots and SDKs use those stable fields to remove
only the rejected market from a batch. The human-readable `error` remains a
diagnostic and is not a machine interface.

Trusted `POST /v1/markets` callers may supply `creation_key` as a stable
operator identity (at most 128 ASCII letters, digits, or `-_:./`). The first
call durably creates the market and commits the key through its metadata
digest. A retry with the same name and creation fields returns the original id
without appending another acknowledged write; the server-generated creation
timestamp is deliberately not a caller field. Reusing the key for a different
contract returns 409. Omitting the key retains allocate-on-every-call behavior.

Market raw price history is served through
`GET /v1/markets/{id}/prices/history`, backed by the private history projector.
The endpoint is bounded by `limit` and pages older committed points with
`before_height` / `next_before_height`.

External reference prices have a separate off-block freshness contract.
`POST /v1/markets/prices/reference` accepts a `prices_nanos` map and records one server receive timestamp per
named market; a partial update refreshes only those names, and zero explicitly
evicts one value. `SYBIL_REFERENCE_PRICE_TTL_MS` defaults to 60 seconds. After
that age, market list, summary, search, and detail responses omit both
`reference_price_nanos` and `reference_price_expires_at_ms`, even if the
publisher process died before it could send an eviction. A present response
includes the exact server expiry so a caching client can enforce the same
boundary between polls. The map is intentionally volatile, so an API restart
also starts with no usable references until the mirror republishes. Arena
refreshes this API view every ten seconds, clears it immediately on refresh
failure, and applies the exact expiry locally; `--require-reference-prices`
therefore cannot select or trade from an expired/pre-republish value.
`sybil_reference_prices_expired_total` and per-market age/available/expired
gauges expose the fail-closed decision; feed-level last-update age remains
distinct from per-token age.

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

Windowed leaderboard reads use a stricter two-sided completeness proof from
the private history status: the projector must begin at genesis or at/before
the requested cutoff, and its contiguous checkpoint timestamp must be at/after
that cutoff. A missing opening equity anchor is treated as post-cutoff account
creation only after both conditions hold. An insufficient floor or lagging
checkpoint returns `503 HISTORY_INCOMPLETE` with a specific reason; the API
does not substitute all-time PnL. The all-time path and a genuinely empty
publishable cohort remain independent of history, and no leaderboard request
adds sequencer actor work because current bases stay in the API-owned read
model.

All account history remains owner-scoped at the public API. Internal history
routes and raw account-attributed batches use a dedicated
`SYBIL_HISTORY_TOKEN`, are private service surfaces, and are not browser
endpoints. Public market history uses the same projector but only committed
market facts. See [[Historical Data Serving]].

Block history reads distinguish missing data from retained-history gaps:
`GET /v1/blocks/{height}` returns `410 Gone` with code `RETENTION_GONE` when
the requested height is below the retained `blocks_full` floor. First-party
WebSocket block replay sends a versioned `retention_gap` envelope before
closing when `?from_block=` starts before durable block retention.

All exchange mutation remains in the `SequencerActor`. Order-write endpoints
first pass through cheap global/per-client HTTP token buckets before JSON or
P256 work. Handlers convert public DTOs to domain instructions and send them to
the actor, which directly admits ordinary supported orders or defers atomic
bundles/MM submissions. Current account reads come from actor snapshots;
committed block reads use the post-commit shared ring and canonical archive
without entering the actor mailbox. Historical product-range queries take the
independent private service path and therefore cannot occupy the sequencer
actor or scan its database.

Actor and persistence availability retain stable retry identity at this
boundary. A missing canonical actor returns `503 SEQUENCER_UNAVAILABLE`;
failure to commit through the sequencer persistence layer returns
`503 SEQUENCER_PERSISTENCE_UNAVAILABLE`. The latter logs its internal cause but
does not expose filesystem/provider details in the response. Integrity
violations remain separate, fail-stop errors rather than availability retries.

The same atomic operational snapshot includes the sequencer's integrity-halt
state. After a hard block invariant fails, `/v1/health` returns `503` with
`status = "integrity_halted"` and the last committed height/genesis for
diagnosis. Canonical writes return `503` with code
`SEQUENCER_INTEGRITY_HALTED`; the actor rejects them before signature work,
nonce advancement, WAL append, or live mutation. Read endpoints continue to
serve the last trusted state. Dev-mode resume cannot clear an integrity halt;
recovery requires an operator restart from the last committed fence.

Cold-start persistence rejection has a narrower surface because no trusted
sequencer snapshot exists to serve. The process enters recovery-only mode with
only `/metrics` and a `503 /v1/health` response whose status is
`restore_failed`; the normal router, reads, streams, and writes are not
mounted. This keeps acknowledged-write restore failures observable while
preventing partial recovery from becoming an API state.

Per-client HTTP buckets use the socket peer by default and ignore forwarding
headers. `SYBIL_HTTP_TRUSTED_PROXY_CIDRS` may name the exact reverse-proxy
networks allowed to supply `X-Forwarded-For`/`X-Real-IP`. For a trusted peer,
the API walks `X-Forwarded-For` from right to left and selects the first
untrusted hop, so caller-supplied addresses to its left cannot spoof the
bucket. Empty configuration is deliberately conservative: traffic behind one
proxy shares that proxy's client bucket.

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
the canonical replay-archive retention behavior, not a new DA policy.
Both retained DA routes share a dedicated global/per-client token bucket and a
hard in-flight concurrency cap before dispatching store work.

The standalone prover consumes two service-token routes backed by the
transactional proof-job outbox. `GET /v1/prover/jobs/next` returns the oldest
unacknowledged job as raw MessagePack with exact height/digest response headers;
`POST /v1/prover/jobs/{height}/ack` records that digest only after the prover
made the bytes durable. Failed acknowledgements deliberately repeat the same
row. In-memory API instances return 503 because they have no durable outbox.

## Key Properties
- Axum-based, async, with OpenAPI auto-generation
- `sybil-openapi` renders the deterministic full public/owner/service/dev
  superset for generated clients; each runtime serves only its mounted profile
- Actor model for current state/mutation; private HTTP projection for history
- All values in [[Nanos and Integer Arithmetic|nanos]] (u64)
- Service bearer auth gates operator writes in production; dev mode is limited to local conveniences and diagnostics
- Order-write endpoints return `429 Too Many Requests` with `Retry-After` under abnormal load
- Integrity-halted canonical writes return `503 SEQUENCER_INTEGRITY_HALTED` while read-only diagnostics retain the last committed identity
- Retained DA reads return `429` when their dedicated rate or concurrency budget is exhausted
- State proof endpoint returns inclusion or exclusion proofs for the latest committed qMDB root
- DA endpoints expose public typed manifests and service-gated retained witness payload bytes
- Public block endpoints expose a typed market-tape projection; canonical account rows are service-only
- Order submissions support GTC, IOC, and GTD time-in-force
- Stateless API layer — all state in sequencer
- CORS is permissive only in dev mode; production uses `SYBIL_CORS_ORIGINS` and defaults to same-origin only
- `GET /v1/attestation` is an unverified shape stub mounted only in dev mode; production returns 404 until real Nitro verification exists

## Where This Lives
> `crates/sybil-api/src/app.rs` — declarative trust-tier route registries, router creation, OpenAPI schema
> `crates/sybil-api/src/routes/` — endpoint handlers
> `crates/sybil-api/src/state.rs` — `AppState` with `SequencerHandle`

## See Also
- [[Order Types]] — the `OrderSpec` enum submitted via the API
- [[WebSocket Block Stream]] — public v2 market tape and authenticated v1 canonical stream
- [[Block Data Boundaries]] — API composition vs. canonical protocol data
- [[P256 Authentication]] — signed order submission
- [[Actor Mailbox Monitoring]] — sequencer queue-depth metric and alerts
- [[Attestation]] — dev-only enclave-attestation shape and the real-verification boundary
- [[Block Lifecycle]] — what happens after an order enters the system
- [[State Root Schema]] — canonical typed-state key/value commitment
