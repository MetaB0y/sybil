---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-04-30
---

The SSE (Server-Sent Events) block stream is the primary way trading bots interact with the exchange in real time. When a client connects to `GET /v1/blocks/stream`, they receive a persistent HTTP connection that pushes each new block as it's produced — fills, clearing prices, rejections, and state updates. This is a one-way channel: the server pushes, the client listens.

SSE was chosen over WebSockets for simplicity: it's unidirectional (server-to-client only, which is exactly what block notifications need), works through HTTP proxies and load balancers without special configuration, automatically reconnects on connection loss, and is natively supported by browser `EventSource` APIs. Bots don't need to send data back over the stream — they submit orders via the [[REST API]]'s `POST /v1/orders` endpoint and then receive the result in the next block via SSE.

The [[Bot Framework]] is built around this pattern. A bot's main loop is `async for block in client.stream_blocks()` — it receives each block, analyzes clearing prices and its own fills, decides on new orders, and submits them via HTTP. The [[Python SDK]] wraps SSE into an async iterator that handles reconnection and parsing. This event-driven architecture means bots are reactive: they respond to market state changes rather than polling for them.

## Key Properties
- `GET /v1/blocks/stream` — persistent HTTP connection with server push
- Each block event includes fills, clearing prices, rejections, state root
- Unidirectional: server → client only
- Auto-reconnect on connection loss
- Primary interaction pattern for [[Bot Framework|trading bots]]
- Simpler than WebSockets: works through proxies, no upgrade handshake

## Where This Lives
> `crates/sybil-api/src/sse.rs` — SSE stream implementation
> `arena/sybil_client/` — `stream_blocks()` async iterator

## See Also
- [[REST API]] — order submission endpoint (the other half of the bot interaction)
- [[WebSocket Block Stream]] — bidirectional production stream with replay and lag signalling
- [[Bot Framework]] — `on_block()` handler driven by SSE events
- [[Python SDK]] — SSE stream wrapped as async iterator
- [[Block Lifecycle]] — blocks pushed to SSE after sealing
