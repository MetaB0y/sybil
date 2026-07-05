---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-06-30
---

The WebSocket block stream is the first-party production transport for the
block feed — a persistent, bidirectional channel at `GET /v1/blocks/ws` that
pushes every committed block to subscribers. It complements the live-only
[[SSE Block Stream]] (`/v1/blocks/stream`) with stronger guarantees: versioned
message envelope, explicit lag signalling with close codes, server-initiated
pings, and gap-free reconnect via `?from_block=N`. These are the properties
long-lived clients (frontends, agents, proof consumers) need; SSE stays around
only as a third-party convenience for scripted tooling and `curl` debugging.

The stream sits on top of a `tokio::sync::broadcast` channel fed by the sequencer actor. Each subscriber gets its own 64-block buffer. If a client falls behind that window, the server sends a final `lagged` envelope and closes the connection with code 1008 — the client reconnects with `?from_block=<last_sent_height + 1>` and the handler replays from block history before switching back to live. The hot in-memory ring is checked first; if the requested height has already been evicted, replay falls back to the durable `blocks_full` store. This is a deliberate "crash fast, recover cleanly" design: no silent block loss, no unbounded buffering.

## Message Envelope

Every message on the wire is JSON with a schema version and a type tag:

```json
{"v": 1, "type": "block", "data": {...BlockResponse}}
{"v": 1, "type": "replay_complete", "up_to_height": 42}
{"v": 1, "type": "lagged", "skipped": 7, "last_sent_height": 42}
{"v": 1, "type": "retention_gap", "requested_height": 10, "retention_min_height": 25, "head_height": 80}
```

- **`block`** — a committed block. Sent during replay and during live streaming. `data` is the same `BlockResponse` shape returned by `GET /v1/blocks/{height}`.
- **`replay_complete`** — sent once after a `?from_block=N` replay finishes. `up_to_height` is the block height at which the server switched from history-replay to the live feed. Anything after this is a live block.
- **`lagged`** — server-side broadcast buffer overflowed. This is the last message on the stream; the server closes the connection with code 1008 immediately after. `last_sent_height` is the highest block the client already received.
- **`retention_gap`** — the requested replay starts below the retained
  `blocks_full` floor. This is the last message on the stream; the server
  closes with code 1008 immediately after. The client must cold-resync because
  the server cannot replay the missing prefix.

Clients should read the `v` field first and ignore messages whose version they don't understand. The server may add new `type` values or additive fields within the same `v`, but any breaking change bumps `v`.

## Reconnect Flow

On disconnect (either a clean `lagged` close or a transport error), a client that has seen block `H` reconnects with `?from_block=H+1`. The server replays every block in `[H+1, current_head]` from hot or durable history, emits `replay_complete`, then switches to the live feed. There is no gap and no duplicate.

Replay reads committed `blocks_full` rows after the hot ring has evicted a
block. If `from_block` is below the retained floor, the server emits
`retention_gap { requested_height, retention_min_height, head_height }` and
closes. Clients can reconnect at `retention_min_height` only after rebuilding
their local state from REST/snapshot data, because blocks below that floor are
not recoverable from this stream.

## Keepalive

The server sends a WebSocket Ping frame every 30 seconds. Any message from the client (including Pong, Ping, or a text frame) counts as liveness. If the server sees no client activity for 90 seconds, it closes with "client idle timeout". Clients should respond promptly to Pings; browser `WebSocket` APIs handle this automatically, but hand-rolled clients need to echo Pings or send their own periodic frames.

## Versioning Policy

- **v1** is the current version. The envelope shape (`v`, `type`, `data`) and current types (`block`, `replay_complete`, `lagged`, `retention_gap`) are frozen.
- **Additive changes within v1**: new `type` values, new optional fields on existing types, new optional query params. Old clients continue to work.
- **Breaking changes**: use a new endpoint path, not a silent change on `/v1/blocks/ws`.

## Where This Lives

> `crates/sybil-api/src/ws.rs` — handler (subscribe, replay, lag detection, ping loop)
> `crates/sybil-api-types/src/ws.rs` — `BlockStreamMessage` / `BlockStreamPayload` schema, shared between server and clients
> `crates/sybil-api/src/routes/blocks.rs` — `/v1/blocks/ws` route wiring
> `crates/sybil-api/tests/ws_integration.rs` — live-block, replay, and from-block-ahead-of-head tests

## See Also
- [[SSE Block Stream]] — simpler alternative at `/v1/blocks/stream`
- [[REST API]] — `GET /v1/blocks/{height}` for one-shot block fetches
- [[Block Lifecycle]] — what's in each `BlockResponse` payload
- [[Historical Data Serving]] — planned durable replay source
