---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-18
---

The WebSocket block stream is the first-party production transport for the
public block feed — a persistent, bidirectional channel at
`GET /v2/blocks/ws` that pushes a privacy-preserving market-tape projection of
every committed block. Its versioned message envelope, explicit lag signalling
with close codes, server-initiated pings, and gap-free reconnect via
`?from_block=N` make it the sole public realtime block transport.

The canonical v1 `BlockResponse`, including account-attributed rows, remains at
`GET /v1/blocks/ws` only for service-authenticated infrastructure. It is not a
public replay protocol. See
[ADR-0016](../../adr/0016-public-market-tape-and-recovery-da-boundaries.md).

The public WebSocket endpoint has a
`SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS` hard cap (default 256). A permit is
held from successful admission until the upgraded connection task exits, and
capacity exhaustion returns HTTP `429` before upgrading. The service-authenticated
v1 stream does not consume this anonymous budget.

The stream sits on top of a `tokio::sync::broadcast` channel fed by the sequencer actor. Each subscriber gets its own 64-block buffer. If a client falls behind that window, the server sends a final `lagged` envelope and closes the connection with code 1008 — the client reconnects with `?from_block=<last_sent_height + 1>` and the handler replays canonical blocks before switching back to live. The recent in-memory cache is checked first; if the requested height has already been evicted, replay falls back to the durable canonical replay archive (physical redb table `blocks_full`). This is a deliberate "crash fast, recover cleanly" design: no silent block loss, no unbounded buffering.

The post-commit recent-block ring is shared with `SequencerHandle` as a
read-only serving surface. Exact-height lookup derives the ring offset in O(1)
and does not enter the sequencer actor mailbox; evicted heights retain the same
durable archive/retention behavior. This matters during correlated reconnects:
ten clients replaying thousands of heights must not enqueue tens of thousands
of reads ahead of block production.

`sybil-ws-load` exercises this boundary with 100 or more public subscribers. It
checks contiguous heights through lag/replay recovery while sampling process
RSS/high-water, sequencer mailbox depth, solve p99, health p95, and committed
height. A normal-cadence target measures fanout capacity; forcing the lag path
requires the disposable fast-cadence profile documented in the
[WebSocket load runbook](../../runbooks/websocket-load.md), because a
10-second feed will not fill normal TCP buffers during a short test. The load
generator is read-only and the suite remains explicit rather than part of fast
CI.

## Message Envelope

Every message on the wire is JSON with a schema version and a type tag:

```json
{"v": 2, "type": "block", "data": {...PublicBlockResponse}}
{"v": 2, "type": "replay_complete", "up_to_height": 42}
{"v": 2, "type": "lagged", "skipped": 7, "last_sent_height": 42}
{"v": 2, "type": "retention_gap", "requested_height": 10, "retention_min_height": 25, "head_height": 80}
```

Every `*_nanos` value inside a block is an exact base-10 JSON string. This
includes each element of `clearing_prices_nanos`; clients should parse these
values as integers/`bigint`, never JavaScript `number`.

- **`block`** — a committed public market-tape row. Sent during replay and live streaming. `data` is the same `PublicBlockResponse` shape returned by `GET /v1/blocks/{height}`: commitments, prices, aggregate analytics, bridge root/count, and sanitized resolved-market ids. Account-bearing canonical rows are absent by type.
- **`replay_complete`** — sent once after a `?from_block=N` replay finishes. `up_to_height` is the block height at which the server switched from history-replay to the live feed. Anything after this is a live block.
- **`lagged`** — server-side broadcast buffer overflowed. This is the last message on the stream; the server closes the connection with code 1008 immediately after. `last_sent_height` is the highest block the client already received.
- **`retention_gap`** — the requested replay starts below the retained
  canonical replay archive floor. This is the last message on the stream; the server
  closes with code 1008 immediately after. The client must cold-resync because
  the server cannot replay the missing prefix.

Clients should read the `v` field first and ignore messages whose version they don't understand. The server may add new `type` values or additive fields within the same `v`, but any breaking change bumps `v`.

## Reconnect Flow

On disconnect (either a clean `lagged` close or a transport error), a client that has seen block `H` reconnects with `?from_block=H+1`. The server replays every block in `[H+1, current_head]` from the recent cache or canonical archive, emits `replay_complete`, then switches to the live feed. There is no gap and no duplicate.

The shared Rust client preserves this distinction through
`stream_block_events_from_block`. Side-effecting consumers must use that event
stream and defer fresh work until `ReplayComplete`; the block-only convenience
stream intentionally hides the boundary and is appropriate only when replaying
a block has the same effect as observing it live. The Polymarket MM, for
example, replays lifecycle and native-price state but never emits historical
quotes.

The Python SDK mirrors that boundary with `stream_block_events()`: block events
carry `replayed`, followed by an explicit replay-complete event.
`stream_blocks()` is the all-block convenience view and
`stream_live_blocks()` filters replay. `BaseAgent` consumes events so replay
can refresh canonical account/fill state without calling `on_block` or
submitting historical orders; the live analyst consumes only live blocks.

When draining a severely backed-up connection, the client may encounter an old
Ping after the server has already queued `lagged` and closed its write side. A
failed late Pong is therefore non-terminal: `sybil-client` keeps reading so the
final versioned envelope wins over an incidental `Broken pipe`. If no envelope
remains, the following read still reports the transport failure normally.

Replay reads the canonical archive (physical redb table `blocks_full`) after
the recent cache has evicted a block. If `from_block` is below the retained floor, the server emits
`retention_gap { requested_height, retention_min_height, head_height }` and
closes. Clients can reconnect at `retention_min_height` only after rebuilding
their local state from REST/snapshot data, because blocks below that floor are
not recoverable from this stream.

The browser client therefore enters a failed/cold-resync state on
`retention_gap`; it does not automatically reconnect. `RealtimeProvider`
clears derived stream state, fetches latest block and market prices from REST,
applies that snapshot, then resumes with
`?from_block=<snapshot_height + 1>`. Independently, the provider owns one
bounded `GET /v1/blocks` bootstrap for global recent-trade and Activity
surfaces. That history read never gates the live handshake, and store merges
are monotonic, so a late history response cannot regress the live head. A
generation fence prevents a pre-recovery response from repopulating a cleared
snapshot. The initial REST-seeded connection follows the same replay
classification until `replay_complete`.

## Keepalive

The server sends a WebSocket Ping frame every 30 seconds. Any message from the client (including Pong, Ping, or a text frame) counts as liveness. If the server sees no client activity for `SYBIL_WS_CLIENT_IDLE_TIMEOUT_MS` (90 seconds by default), it closes with "client idle timeout". Clients should respond promptly to Pings; browser `WebSocket` APIs handle this automatically, but hand-rolled clients need to echo Pings or send their own periodic frames. Disposable recovery tests may lengthen the window so a deliberate no-read stall reaches the independent lag boundary; shared deployments keep the production default.

## Versioning Policy

- **Public v2** is the current public version. The envelope shape (`v`, `type`, `data`) and current types (`block`, `replay_complete`, `lagged`, `retention_gap`) are frozen.
- **Additive changes within v2** must stay inside the public allowlist; a canonical/private field is never an additive public change.
- **Service v1** preserves the former full canonical DTO behind service authentication.
- **Breaking public changes** use a new endpoint path, not a silent change on `/v2/blocks/ws`.

## Where This Lives

> `crates/sybil-api/src/ws.rs` — handler (subscribe, replay, lag detection, ping loop)
> `crates/sybil-api-types/src/ws.rs` — public v2 and service v1 envelope schemas
> `crates/sybil-api/src/routes/blocks.rs` — `/v2/blocks/ws` public and `/v1/blocks/ws` service wiring
> `crates/sybil-api/tests/ws_integration.rs` — live-block, replay, retention-gap, and connection-cap tests
> `crates/sybil-loadtest/src/bin/ws_load.rs` — explicit 100+ subscriber capacity/recovery harness

## See Also
- [[REST API]] — `GET /v1/blocks/{height}` for one-shot block fetches
- [[Block Lifecycle]] — canonical block production behind the public projection
- [[Historical Data Serving]] — durable replay and query ownership
