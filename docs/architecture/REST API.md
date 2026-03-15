---
tags: [infrastructure, crate]
layer: api
crate: sybil-api
status: current
last_verified: 2026-03-15
---

The REST API is the external interface to the exchange. Built with Axum (a Rust async web framework), it exposes endpoints for account management, market operations, order submission, and block retrieval. An OpenAPI schema is auto-generated for client code generation. The API communicates with the sequencer via message passing through a `SequencerHandle` — no shared mutable state.

The endpoint groups are: **System** (`/v1/health`, `/v1/state-root`), **Accounts** (create, query balance/positions, fund, register keys), **Markets** (list, create, query details/prices/groups, resolve), **Orders** (submit unsigned or [[P256 Authentication|signed]]), and **Blocks** (latest, by height, [[SSE Block Stream|SSE stream]]). Many endpoints are dev-mode only: account creation/funding, market creation/resolution, and group creation. In production, these administrative operations would be restricted to governance or oracle processes.

The API is stateless — all state lives in the `SequencerActor`. When an order submission arrives at `POST /v1/orders`, the API handler converts the [[Order Types|OrderSpec]] to the engine's internal representation and sends it to the sequencer via a channel. The sequencer enqueues it in the [[Mempool]]. When a client queries `GET /v1/blocks/latest`, the API reads the latest sealed block from the sequencer's state. This actor-model architecture (inspired by Tokio actor patterns) means the API layer can be horizontally scaled without coordination — all instances talk to the same sequencer actor.

## Key Properties
- Axum-based, async, with OpenAPI auto-generation
- Actor model: API → message channel → `SequencerActor`
- All values in [[Nanos and Integer Arithmetic|nanos]] (u64)
- Dev mode gates administrative endpoints
- Stateless API layer — all state in sequencer
- CORS enabled for browser-based clients

## Where This Lives
> `crates/sybil-api/src/app.rs` — router creation, OpenAPI schema
> `crates/sybil-api/src/routes/` — endpoint handlers
> `crates/sybil-api/src/state.rs` — `AppState` with `SequencerHandle`

## See Also
- [[Order Types]] — the `OrderSpec` enum submitted via the API
- [[SSE Block Stream]] — real-time block push via `/v1/blocks/stream`
- [[P256 Authentication]] — signed order submission
- [[Block Lifecycle]] — what happens after an order enters the system
