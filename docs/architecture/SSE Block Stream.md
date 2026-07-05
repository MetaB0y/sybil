---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-04-30
---

The SSE (Server-Sent Events) block stream is a third-party convenience endpoint
for simple tooling and HTTP-native consumers. When a client connects to
`GET /v1/blocks/stream`, they receive a persistent HTTP connection that pushes
each new block as it's produced — fills, clearing prices, rejections, and state
updates. This is a one-way channel: the server pushes, the client listens.

First-party clients use the [[WebSocket Block Stream]] at
`GET /v1/blocks/ws?from_block=N`. The WebSocket transport is versioned, can
replay retained committed blocks from a requested height, signals lag
explicitly, and returns a `retention_gap` envelope when `from_block` is below
the retained `blocks_full` floor. SSE intentionally does not provide those
resume guarantees: it is a thin live re-encoding of the same block broadcast.

The endpoint remains useful for `curl`, quick scripts, and third-party clients
that prefer plain HTTP streams over a WebSocket upgrade. Long-lived Sybil-owned
clients should reconnect with WebSocket `?from_block=<last_seen_height + 1>`
instead.

## Key Properties
- `GET /v1/blocks/stream` — third-party convenience HTTP stream with server push
- Each block event includes fills, clearing prices, rejections, state root, and events root
- Unidirectional: server → client only
- No versioned envelope, replay cursor, lag signal, or retained-floor contract
- First-party clients use `GET /v1/blocks/ws?from_block=N`
- Simple proxy-friendly stream for external tooling

## Where This Lives
> `crates/sybil-api/src/sse.rs` — SSE stream implementation
> third-party scripts and generated clients that still prefer SSE

## See Also
- [[REST API]] — order submission endpoint (the other half of the bot interaction)
- [[WebSocket Block Stream]] — bidirectional production stream with replay and lag signalling
- [[Bot Framework]] — `on_block()` handler driven by SSE events
- [[Python SDK]] — SSE stream wrapped as async iterator
- [[Block Lifecycle]] — blocks pushed to SSE after sealing
