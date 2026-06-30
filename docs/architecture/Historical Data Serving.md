---
tags: [infrastructure, storage]
layer: sequencer
status: planned
last_verified: 2026-06-30
---

# Historical Data Serving

Sybil needs durable historical data without turning the sequencer actor into an unbounded cache. The current runtime keeps a small recent block ring and bounded per-market price history in memory. That is acceptable for live clients, but it cannot be the source of truth for replay, backtesting, charts, or post-restart recovery.

The design goal is a clear hot/cold boundary:

- The sequencer actor owns live exchange state and recent notifications.
- redb/qMDB own committed recovery state.
- A durable history boundary owns append-only block and analytics history.
- API queries are paginated range scans, not full in-memory dumps.

## Tier 1: Store-Backed History

The next implementation step should keep history inside the sequencer process but outside actor memory. This is the simplest correct shape:

- Persist every canonical sealed block by height.
- Persist the block analytics sidecar by height.
- Persist price points keyed by `(market_id, height)` or `(market_id, timestamp_ms, height)`.
- Keep the existing in-memory ring as a hot cache only.
- On cache miss, `GET /v1/blocks/{height}` and WebSocket replay read from the durable block table.
- Price history endpoints read bounded ranges from the price table.

This keeps the block commit boundary simple: when a block is saved, its replay payload and derived history rows are saved in the same redb transaction as the other block metadata.

## Schema Sketch

Suggested redb tables:

| Table | Key | Value |
|-------|-----|-------|
| `blocks_by_height` | `height` | canonical `SealedBlock` or API-neutral block payload |
| `block_sidecars_by_height` | `height` | analytics sidecar |
| `price_points_by_market_height` | `(market_id, height)` | clearing/reference price point |
| `history_cursors` | name | pruning and export cursors |

The store may retain only serialized canonical structures initially. If query CPU becomes a problem, add read-optimized projections later. Do not make the actor keep extra copies to avoid serialization work.

## API Contract

Historical APIs should be explicit and bounded:

- `GET /v1/blocks?after_height=&before_height=&limit=`
- `GET /v1/blocks/{height}`
- `GET /v1/markets/{id}/prices/history?after_height=&before_height=&limit=&resolution=`

Defaults should be small. Maximum limits should be enforced server-side. Responses should include pagination cursors so clients do not ask for "all history".

WebSocket reconnect with `?from_block=N` should first use the hot ring. If `N` is older than the ring, it should stream from durable block history up to the current head and then switch to live broadcast.

## Retention and Downsampling

Unbounded retention is a product decision, not an accident. The default policy should be:

- Keep canonical blocks for a configured number of heights or days.
- Keep raw price points for a shorter window.
- Keep downsampled price candles for longer windows.
- Expose the effective retention window through health or metadata.

If blocks are needed for verification or legal/audit reasons, canonical block retention should be longer than UI chart retention.

## When To Split A History Service

A separate history service becomes worthwhile when history queries materially compete with block production. The service would subscribe to the durable block stream or read committed block rows, build OLAP-friendly projections, and serve chart/backtest queries independently.

The split is not required for correctness, and adding it before the store boundary exists creates more moving parts without solving data loss. The clean sequence is:

1. Persist canonical block and price history at commit time.
2. Serve bounded paginated history from the store.
3. Add export cursors.
4. Split history serving only when load or query shape demands it.

## Test Requirements

- Restart tests proving `GET /v1/blocks/latest` and `GET /v1/blocks/{height}` work immediately after restore.
- WebSocket replay tests where `from_block` is older than the in-memory ring but still inside durable retention.
- Price-history tests across restart with more points than the in-memory cap.
- Retention tests proving old rows are pruned only according to configured policy.
- Load tests proving large history ranges are paginated and do not allocate unbounded vectors.

## Related Notes

- [[Persistence]] - recovery state and current volatile caches
- [[Block Data Boundaries]] - canonical blocks versus analytics sidecars
- [[WebSocket Block Stream]] - replay behavior
- [[REST API]] - endpoint surface
