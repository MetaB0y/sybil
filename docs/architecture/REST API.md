---
tags: [infrastructure, crate]
layer: api
crate: sybil-api
status: current
last_verified: 2026-04-26
---

The REST API is the external interface to the exchange. Built with Axum (a Rust async web framework), it exposes endpoints for account management, market operations, order submission, and block retrieval. An OpenAPI schema is auto-generated for client code generation. The API communicates with the sequencer via message passing through a `SequencerHandle` — no shared mutable state.

The endpoint groups are: **System** (`/v1/health`, `/v1/state-root`), **Accounts** (create, query balance/positions, fund, register keys), **Markets** (list, create, query details/prices/groups, resolve), **Orders** (submit unsigned or [[P256 Authentication|signed]]), and **Blocks** (latest, by height, [[SSE Block Stream|SSE stream]]). Many endpoints are dev-mode only: account creation/funding, market creation/resolution, and group creation. In production, these administrative operations would be restricted to governance or oracle processes.

The API is stateless — all exchange state lives in the `SequencerActor`. Order-write endpoints first pass through a cheap HTTP token bucket keyed globally and by client address/header. This happens before JSON parsing and P256 signature verification, so malformed or invalid signed traffic cannot consume unbounded CPU. When an order submission arrives at `POST /v1/orders`, the API handler converts the [[Order Types|OrderSpec]] to the engine's internal representation and sends it to the sequencer via a channel. The sequencer either directly admits simple single-market orders into the resting book or defers MM / bundle / multi-market submissions via the [[Mempool|deferred-submission buffer]]. When a client queries `GET /v1/blocks/latest`, the API reads the latest sealed block from the sequencer's state. This actor-model architecture (inspired by Tokio actor patterns) means the API layer can be horizontally scaled without coordination — all instances talk to the same sequencer actor.

The sequencer message path is monitored by [[Actor Mailbox Monitoring]]. The API exports `sybil_actor_queue_depth{actor="sequencer"}` from `/metrics`, with configurable warning and critical thresholds, so operator alerts distinguish actor backlog from solver latency or HTTP rejection pressure.

## Key Properties
- Axum-based, async, with OpenAPI auto-generation
- Actor model: API → message channel → `SequencerActor`
- All values in [[Nanos and Integer Arithmetic|nanos]] (u64)
- Dev mode gates administrative endpoints
- Order-write endpoints return `429 Too Many Requests` with `Retry-After` under abnormal load
- Stateless API layer — all state in sequencer
- CORS enabled for browser-based clients

## Where This Lives
> `crates/sybil-api/src/app.rs` — router creation, OpenAPI schema
> `crates/sybil-api/src/routes/` — endpoint handlers
> `crates/sybil-api/src/state.rs` — `AppState` with `SequencerHandle`

## See Also
- [[Order Types]] — the `OrderSpec` enum submitted via the API
- [[SSE Block Stream]] — real-time block push via `/v1/blocks/stream`
- [[WebSocket Block Stream]] — production block stream at `/v1/blocks/ws` with replay + backpressure
- [[P256 Authentication]] — signed order submission
- [[Actor Mailbox Monitoring]] — sequencer queue-depth metric and alerts
- [[Block Lifecycle]] — what happens after an order enters the system
