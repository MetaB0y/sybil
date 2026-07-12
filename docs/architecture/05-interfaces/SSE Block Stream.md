---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-12
---

The SSE (Server-Sent Events) block stream is a third-party convenience endpoint
for simple tooling and HTTP-native consumers. When a client connects to
`GET /v1/blocks/stream`, they receive a persistent HTTP connection that pushes
each new block's public market-tape projection as it's produced: commitments,
clearing prices, aggregate statistics, bridge root/count, and sanitized market
resolution ids. Account-attributed fills, rejections, and lifecycle rows do not
exist in this DTO. This is a one-way channel: the server pushes, the client
listens.

First-party clients use the [[WebSocket Block Stream]] at
`GET /v2/blocks/ws?from_block=N`. The WebSocket transport is versioned, can
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
- Each block event includes commitments, clearing prices, safe lifecycle ids, and aggregate analytics
- Individual fills, rejections, accounts, keys, and bridge leaves are absent by type
- Unidirectional: server → client only
- No versioned envelope, replay cursor, lag signal, or retained-floor contract
- First-party public clients use `GET /v2/blocks/ws?from_block=N`
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
