---
tags: [arena, crate]
layer: arena
status: current
last_verified: 2026-07-22
---

The Python SDK (`sybil_client`) is an async client that wraps the [[REST API]] for bot development. Built on `httpx` and `websockets`, it provides a `SybilClient` class with methods for every API endpoint plus convenience features: automatic [[Nanos and Integer Arithmetic|nanos]] conversion (pass prices as floats like 0.55 instead of 550,000,000), resumable WebSocket block streaming as an async iterator, and typed response objects.

The core interaction pattern is straightforward. Create a client with the server URL, create an account with initial funds, submit orders using helper functions like `buy_yes(account_id, market_id, price, quantity)`, and stream blocks to see results. The SDK handles nanos and share-unit conversion — you think in dollars and ordinary shares, while the wire sends nanos and fixed-point quantity units (`1000` units = 1 share). `stream_block_events()` preserves whether a block is replayed and exposes the replay-complete boundary. `stream_blocks()` yields every block for idempotent observers; `stream_live_blocks()` yields only blocks safe for fresh side effects. Reconnects resume from the last delivered height; a retained-history gap raises `BlockStreamGapError` and requires a cold resync.

The SDK is intentionally thin — it's a transport layer, not a trading framework. Strategy logic, position tracking, risk management, and decision-making live in the [[Bot Framework]] and individual bot implementations. The SDK just moves data between Python and the Rust server. It supports unsigned orders (dev mode), [[P256 Authentication|signed orders]], and transport of externally signed atomic MM-bundle submit/replace/cancel actions.

`submit_signed_mm_bundle()` preserves every protocol integer exactly: bundle
revision, order price/quantity/expiry, maximum-capital nanodollars, and nonce.
It accepts the already-produced RawP256/WebAuthn envelope rather than owning a
private key or reconstructing signing bytes. Canonical MM-bundle construction
and signing remain owned by `sybil-signing`; generated Python models are
regenerated from OpenAPI.

`get_order_admission_policy()` exposes the server's typed
`OrderAdmissionPolicy`. The wrapper preserves the minimum notional as an exact
integer and rejects a server/client share-scale mismatch. It does not resize
orders or make a risk decision; the [[Bot Framework]] owns that policy.

## Key Properties
- Async `httpx`/WebSocket client: `SybilClient`
- Automatic nanos conversion: pass prices as floats (0.55 → 550,000,000)
- Automatic share-unit conversion: pass quantities as shares (1.5 → 1500 units on the wire)
- `stream_blocks()` — resumable WebSocket wrapped as Python async iterator
- `stream_block_events()` — replay-aware event stream for stateful consumers
- `stream_live_blocks()` — live-only convenience stream for side effects
- Typed response objects: `Account`, `Market`, `Block`, `Fill`
- Order helpers: `BuyYes`, `BuyNo`, `SellYes`, `SellNo`
- Exact transport for externally signed atomic MM-bundle lifecycle actions
- Typed public order-construction policy with exact nanos and unit-drift rejection
- Thin transport layer — no strategy logic

The vendored `_generated` package is rendered from the deterministic full
`sybil-openapi` document, so it contains public, owner, service, and dev
operation modules. Runtime deployments still mount only their configured
profile; generation completeness does not grant runtime access.

## Where This Lives
> `arena/sybil_client/` — `SybilClient` class, response types, order specs

## See Also
- [[REST API]] — the HTTP endpoints the SDK wraps
- [[WebSocket Block Stream]] — gap-aware real-time block push consumed by `stream_blocks()`
- [[Bot Framework]] — the strategy layer built on top of the SDK
