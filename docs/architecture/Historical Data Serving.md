---
tags: [infrastructure, storage]
layer: sequencer
status: planned
last_verified: 2026-07-01
---

# Historical Data Serving

Sybil needs durable historical data without turning the sequencer actor into an
unbounded cache. The current runtime keeps a small recent block ring and bounded
per-market price history in memory. That is acceptable for live clients, but it
cannot be the source of truth for replay, backtesting, charts, or post-restart
recovery.

The design goal is a clear hot/cold boundary:

- The sequencer actor owns live exchange state and recent notifications.
- redb/qMDB own committed recovery state.
- A durable history boundary owns append-only block and analytics history.
- API queries are paginated range scans, not full in-memory dumps.

## Decision

Build the durable store boundary first, inside the existing API/sequencer
process, then split history into a separate service only if read load or query
shape demands it.

This avoids two failure modes:

- Raising in-memory caps until the 2 GB Linode OOMs.
- Creating a second service that still has no durable committed history source.

## Phase 1: Store-Backed History

- Persist every canonical sealed block by height.
- Persist every block analytics sidecar by height.
- Persist every emitted mark-price point keyed by `(market_id, height)`.
- Keep the existing in-memory ring as a hot cache only.
- On cache miss, `GET /v1/blocks/{height}` reads from durable block history.
- `GET /v1/blocks` pages durable summaries rather than walking the hot ring.
- WebSocket replay reads durable blocks when `from_block` is older than the hot ring.
- Price-history endpoints read bounded ranges from the price table.

This keeps the block commit boundary simple: when a block is saved, its replay
payload and derived history rows are saved in the same redb transaction as the
other block metadata and qMDB fence flip. A block is historical only if the same
transaction that commits the block also commits its history rows.

Phase 1 does not require a separate process, async export pipeline, object
storage, or OLAP database.

Implemented so far:

- `blocks_full` persists full `SealedBlock` replay payloads by height from the
  actor commit path.
- `GetBlock(height)` checks the hot in-memory ring first, then falls back to
  `blocks_full`.
- WebSocket replay with `from_block` uses durable block rows when the requested
  replay starts before the hot ring.
- `price_points` persists raw mark-price rows by `(market_id, height)` from the
  same block commit transaction.
- `GetPriceHistory(market_id, from_ms, to_ms, limit)` uses the durable price
  table when a store is configured. The public endpoint defaults to 500 points
  and clamps requests at 5,000 points.
- Regression tests cover ring eviction plus cold restart for exact block reads,
  WebSocket replay, and raw price-history reads.

Still planned in this phase:

- `block_summaries` and paginated `GET /v1/blocks`.
- Cursor metadata for price-history pagination.
- Retention metadata, pruning, and downsampled candles.

## Schema Sketch

Suggested redb tables for Phase 1:

| Table | Key | Value |
|-------|-----|-------|
| `blocks_full` | `height: u64` | versioned `SealedBlock` replay payload |
| `block_summaries` | `height: u64` | compact block row for list pages |
| `price_points` | `(market_id: u32, height: u64)` | `PricePoint` mark row |
| `history_meta` | string key | min/max retained height, prune/export cursors |

`blocks_full` should store the versioned API-neutral replay payload needed by
REST and WebSocket catch-up. It can start as `SealedBlock` if that is the
lowest-risk implementation, but the table name should not promise consensus
state only: API replay needs canonical block fields plus the analytics sidecar
currently carried by `BlockResponse`.

`block_summaries` exists so list pages do not deserialize full fills, prices,
rejections, and per-market sidecars for every row. It should contain only fields
needed by `GET /v1/blocks`: height, timestamp, order/fill counts, volume,
welfare, state root, parent hash, and maybe per-market counts if the current UI
requires them.

`price_points` must persist the same mark series that the frontend chart uses
today: clearing price when a market trades, book midpoint when it does not trade
but the book moves, and carry-over otherwise. Persisting only solver clearing
prices would not preserve quiet-market charts or 24h deltas.

Keys should be big-endian fixed-width bytes when compound ordering matters:

```text
price_points key = be_u32(market_id) || be_u64(height)
```

The store may retain serialized structures initially. If query CPU becomes a
problem, add read-optimized projections later. Do not make the actor keep extra
copies to avoid serialization work.

## Write Path

Block commit should remain the only history commit point:

1. `prepare_block` builds the next sequencer and `BlockProduction`.
2. `persist_block` calls `Store::save_block_with_witness(...)` with the prepared
   snapshot, witness, sealed block payload, compact summary, and price-point
   delta.
3. `save_block` writes qMDB inactive slots, redb state, full block, summary,
   price rows, and retention metadata in one redb transaction.
4. Only after the transaction succeeds does the actor swap in the prepared
   sequencer and broadcast the sealed block.

If the process crashes before redb commits, the block and its history do not
exist. If it crashes after redb commits, both recovery state and history rows
exist. There should be no state where `/v1/health` reports a committed height
that cannot be loaded by `GET /v1/blocks/{height}`.

Price rows should be append-only deltas emitted by `PriceTracker::record_block`.
The in-memory `price_history` cache remains bounded; the durable table is the
source of truth for historical ranges.

## Read Path

Historical reads must be bounded before they allocate:

- `GetBlock(height)` can check the hot ring first, then read `blocks_full`.
- `GetRecentBlocks(limit)` should become a store-backed page, not a full
  in-memory ring dump.
- `GetPriceHistory(market_id, range, limit)` is a range scan over
  `price_points` with a hard cap. Today the endpoint supports a bounded `limit`;
  it still needs cursor metadata before clients can page long ranges precisely.
- WebSocket replay should page durable blocks in small chunks before switching
  to live broadcast.

The actor may broker these reads initially for simplicity, but it must never
hold unbounded result sets. If redb reads inside the actor start competing with
block production, move history reads behind a cloneable store/history reader
owned by API state while keeping writes sequencer-owned.

## API Contract

Historical APIs should be explicit and bounded:

- `GET /v1/blocks?before_height=&after_height=&limit=`
- `GET /v1/blocks/{height}`
- `GET /v1/markets/{id}/prices/history?from_ms=&to_ms=&after_height=&before_height=&limit=&resolution=`

Defaults should be small. Maximum limits should be enforced server-side.
Responses should include pagination cursors so clients do not ask for "all
history".

Suggested caps:

| Endpoint | Default | Max |
|----------|---------|-----|
| `GET /v1/blocks` | 60 | 500 |
| `GET /v1/blocks/ws?from_block=` durable replay page | 64 internal | 64 internal |
| `GET /v1/markets/{id}/prices/history` raw points | 500 | 5000 |
| Downsampled/candled price history | 500 | 2000 |

WebSocket reconnect with `?from_block=N` should first use the hot ring. If `N`
is older than the ring, it should stream from durable block history up to the
current head and then switch to live broadcast. If `N` is older than durable
retention, it should fail clearly with a retention/gap envelope rather than
silently skipping blocks.

## Retention and Downsampling

Unbounded retention is a product decision, not an accident. The default policy should be:

- Keep canonical blocks for a configured number of heights or days.
- Keep raw price points for a shorter window.
- Keep downsampled price candles for longer windows.
- Expose the effective retention window through health or metadata.

If blocks are needed for verification or legal/audit reasons, canonical block retention should be longer than UI chart retention.

Initial devnet defaults can be conservative:

- Canonical blocks: 7 days or 100,000 blocks, whichever is lower.
- Raw mark price points: 7 days.
- Candles: planned but not required for first correctness pass.

Pruning must be explicit and metadata-driven. A prune should advance
`history_meta.min_block_height` and `history_meta.min_price_height` only after
the relevant rows are deleted. API responses should expose the retained range so
clients can tell "outside retention" apart from "no data".

Do not prune account event history, fills, or equity as part of this work; those
have their own pagination and retention decisions.

## When To Split A History Service

A separate history service becomes worthwhile when history queries materially
compete with block production. The service would subscribe to the durable block
stream or read committed block rows, build OLAP-friendly projections, and serve
chart/backtest queries independently.

The split is not required for correctness, and adding it before the store boundary exists creates more moving parts without solving data loss. The clean sequence is:

1. Persist canonical block and price history at commit time.
2. Serve bounded paginated history from the store.
3. Add export cursors.
4. Split history serving only when load or query shape demands it.

Split triggers:

- p95 block production latency regresses when history endpoints are used.
- Price/history queries need aggregations that redb range scans cannot serve
  cheaply.
- Backtesting/export workloads need large scans that should not run in the API
  process.
- The retention target grows beyond what the devnet host should keep in redb.

The split service should treat redb history rows as its input log. It should not
be the first durable store.

## Test Requirements

- Restart tests proving `GET /v1/blocks/latest` and `GET /v1/blocks/{height}`
  work immediately after restore.
- A ring-overflow test with block ring capacity `N`, produce `N + M` blocks,
  restart, and verify old blocks inside durable retention still serve.
- WebSocket replay tests where `from_block` is older than the in-memory ring but
  still inside durable retention.
- WebSocket replay tests where `from_block` is older than retention and returns
  an explicit gap/retention response.
- Price-history tests across restart with more points than the in-memory cap.
- Price-history tests proving quiet-market midpoint marks persist, not only
  traded clearing prices.
- Retention tests proving old rows are pruned only according to configured
  policy.
- Load tests proving large history ranges are paginated and do not allocate
  unbounded vectors.

## Linear Implementation Split

- SYB-146: persist sealed block history and exact-height fallback.
- SYB-147: replay WebSocket block stream from durable history.
- SYB-148: persist paginated market price history with retention policy.
- SYB-156: keep `frontend/DATA_MAP.md` aligned as these contracts change.

## Related Notes

- [[Persistence]] - recovery state and current volatile caches
- [[Block Data Boundaries]] - canonical blocks versus analytics sidecars
- [[WebSocket Block Stream]] - replay behavior
- [[REST API]] - endpoint surface
