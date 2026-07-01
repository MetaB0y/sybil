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
transaction that commits the block also commits its history rows. Retention
pruning is separate bounded maintenance: it may lag behind the retention target,
but it must never advertise a row as pruned before the delete transaction
commits.

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
- `GetPriceHistory(market_id, from_ms, to_ms, before_height, limit)` uses the
  durable price table when a store is configured. The public endpoint defaults
  to 500 points and clamps requests at 5,000 points. `before_height` /
  `next_before_height` provide older-page cursoring.
- Regression tests cover ring eviction plus cold restart for exact block reads,
  WebSocket replay, and raw price-history reads.

Still planned in this phase:

- `block_summaries` and paginated `GET /v1/blocks`.
- Retention metadata, pruning, and downsampled candles.

## Schema Sketch

Suggested redb tables for Phase 1:

| Table | Key | Value |
|-------|-----|-------|
| `blocks_full` | `height: u64` | versioned `SealedBlock` replay payload |
| `block_summaries` | `height: u64` | compact block row for list pages |
| `price_points` | `(market_id: u32, height: u64)` | `PricePoint` mark row |
| `price_candles` | `(market_id: u32, resolution_secs: u32, bucket_start_ms: u64)` | `PriceCandle` OHLCV row |
| `history_meta` | string key | retained floors, prune cursors, export cursors |

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

`price_candles` are derived product rows for long-window charting. They are not
canonical exchange state. They should be written from the same block-local
price-point delta as `price_points`, so a committed block cannot have raw rows
without matching candle updates for configured resolutions.

Keys should be big-endian fixed-width bytes when compound ordering matters:

```text
price_points key = be_u32(market_id) || be_u64(height)
price_candles key = be_u32(market_id) || be_u32(resolution_secs) || be_u64(bucket_start_ms)
```

The store may retain serialized structures initially. If query CPU becomes a
problem, add read-optimized projections later. Do not make the actor keep extra
copies to avoid serialization work.

## Write Path

Block commit should remain the only history commit point:

1. `prepare_block` builds the next sequencer and `BlockProduction`.
2. `persist_block` calls `Store::save_block_with_witness_and_history(...)` with
   the prepared snapshot, witness, sealed block payload, compact summary, and
   price-point delta.
3. `save_block` writes qMDB inactive slots, redb state, full block, summary,
   raw price rows, and candle rows in one redb transaction.
4. Only after the transaction succeeds does the actor swap in the prepared
   sequencer and broadcast the sealed block.

If the process crashes before redb commits, the block and its history do not
exist. If it crashes after redb commits, both recovery state and history rows
exist. There should be no state where `/v1/health` reports a committed height
that cannot be loaded by `GET /v1/blocks/{height}`.

Price rows should be append-only deltas emitted by `PriceTracker::record_block`.
The in-memory `price_history` cache remains bounded; the durable table is the
source of truth for historical ranges.

Retention should not run as unbounded work inside this transaction. After a
successful block commit, the actor or a maintenance task may call
`Store::prune_history(policy, budget)` in a separate bounded redb write
transaction. If pruning is interrupted, extra rows remain available; correctness
does not depend on pruning catching up immediately.

## Read Path

Historical reads must be bounded before they allocate:

- `GetBlock(height)` can check the hot ring first, then read `blocks_full`.
- `GetRecentBlocks(limit)` should become a store-backed page, not a full
  in-memory ring dump.
- `GetPriceHistory(market_id, range, limit)` is a range scan over
  `price_points` with a hard cap and `before_height` cursoring.
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
- `GET /v1/markets/{id}/prices/history?from_ms=&to_ms=&before_height=&limit=`
- `GET /v1/markets/{id}/prices/candles?resolution=&from_ms=&to_ms=&before_ms=&limit=`

Defaults should be small. Maximum limits should be enforced server-side.
Responses should include pagination cursors so clients do not ask for "all
history".

Suggested caps:

| Endpoint | Default | Max |
|----------|---------|-----|
| `GET /v1/blocks` | 60 | 500 |
| `GET /v1/blocks/ws?from_block=` durable replay page | 64 internal | 64 internal |
| `GET /v1/markets/{id}/prices/history` raw points | 500 | 5000 |
| `GET /v1/markets/{id}/prices/candles` | 500 | 2000 |

WebSocket reconnect with `?from_block=N` should first use the hot ring. If `N`
is older than the ring, it should stream from durable block history up to the
current head and then switch to live broadcast. If `N` is older than durable
retention, it should fail clearly with a retention/gap envelope rather than
silently skipping blocks.

## Retention and Pruning

Unbounded retention is a product decision, not an accident. The first production
policy should be simple and explicit:

- Keep full replay blocks for a configured height window.
- Keep raw mark-price points for a configured height or time window.
- Keep price candles for a longer configured time window.
- Expose retained floors in API responses and health/metadata so clients can
  distinguish "outside retention" from "no data".

Initial devnet defaults should be conservative and operator-tunable. A useful
starting point is:

| Data | Default | Rationale |
|------|---------|-----------|
| `blocks_full` | 100,000 blocks | Enough for reconnect/replay debugging without retaining every historical payload forever. |
| `price_points` raw rows | 7 days or 1,000,000 blocks, whichever is lower | Raw charts stay useful for recent investigation; old chart windows should use candles. |
| 1 minute candles | 30 days | UI-friendly medium horizon. |
| 5 minute candles | 180 days | Long horizon without a row explosion. |
| 1 hour candles | unbounded on devnet until size proves otherwise | Cheap enough for coarse historical charts and backtests. |

These are not consensus constants. They belong in runtime configuration, for
example:

- `SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS`
- `SYBIL_RAW_PRICE_RETENTION_BLOCKS`
- `SYBIL_PRICE_CANDLE_RETENTION_BLOCKS`
- `SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS`
- `SYBIL_HISTORY_PRUNE_MAX_ROWS`

Use block-height windows for `blocks_full` and raw `price_points` first. They
are deterministic, cheap to compare, and match current keys. Time-based raw
retention can be layered later if product requirements need wall-clock windows
across variable block cadence.

### Metadata

Add a small `history_meta` table rather than overloading generic counters.
Values can start as `u64`; use MessagePack only when a field needs structure.

Suggested keys:

| Key | Meaning |
|-----|---------|
| `blocks_full_min_height` | Lowest full block height still expected to serve. |
| `price_points_min_height` | Lowest raw price-point height still expected to serve. |
| `price_candles_min_bucket_ms:{resolution_secs}` | Lowest retained candle bucket per resolution. |
| `last_history_prune_height` | Last committed block height that attempted pruning. |

Metadata must trail reality, never lead it. A prune transaction should:

1. Delete rows below the configured floor, up to the per-run budget.
2. Inspect or compute the lowest remaining retained row for that stream.
3. Advance the corresponding `history_meta` floor in the same transaction.
4. Commit.

If a crash happens before commit, neither deletes nor metadata changes are
visible. If a crash happens after commit, both are visible. If the budget is
exhausted, the floor advances only as far as rows were actually deleted.

redb may not shrink its file immediately after row deletes. Retention controls
logical history growth and future page reuse; physical compaction should be a
separate low-frequency maintenance action, not a per-block operation.

### API Gap Semantics

APIs should report retention explicitly:

- `GET /v1/blocks/{height}` should return a distinct retention/gone error when
  `height < blocks_full_min_height`, not a generic not-found response.
- `GET /v1/blocks/ws?from_block=N` should send a versioned retention-gap
  envelope and close cleanly when `N < blocks_full_min_height`.
- Raw price-history responses should include `retention_min_height` once
  retention is active.
- Candle responses should include `retention_min_bucket_ms` for the selected
  resolution.

"No rows matched inside retention" and "the requested range is older than
retention" are different product states. The frontend should be able to show
them differently.

Do not prune account event history, fills, or equity as part of this work. Those
have their own pagination and retention decisions.

## Price Candles

Candles are the first downsampled history product. They should be boring:
deterministic OHLCV rows derived from committed raw mark points.

Suggested value:

```text
PriceCandle {
  bucket_start_ms,
  bucket_end_ms,
  first_height,
  last_height,
  open_yes_price,
  high_yes_price,
  low_yes_price,
  close_yes_price,
  open_no_price,
  high_no_price,
  low_no_price,
  close_no_price,
  volume_nanos,
  point_count,
}
```

The write path should update candle rows from `price_points_delta` in the same
store transaction that appends raw rows. For each configured resolution, compute
`bucket_start_ms = timestamp_ms - (timestamp_ms % resolution_ms)` and merge the
point into the row:

- `open_*` and `first_height` come from the earliest point in the bucket.
- `close_*` and `last_height` come from the latest point in the bucket.
- `high_*` and `low_*` are max/min over points in the bucket.
- `volume_nanos` sums block-local raw-point volumes.
- `point_count` increments by one per raw point merged.

Do not store synthetic empty candles. Sparse markets should have sparse candle
rows; chart clients can carry the last close visually when they need step-line
continuity. Storing empty rows for every market and bucket would recreate the
unbounded growth problem in a quieter form.

Expose candles through a distinct endpoint rather than overloading the raw
price-point schema:

```text
GET /v1/markets/{id}/prices/candles?resolution=1m&from_ms=&to_ms=&before_ms=&limit=
```

This keeps existing raw chart clients stable and gives candle responses their
own cursor, retention metadata, and OHLC fields.

Backfill is optional. After deployment, candle rows can start from the first
new committed block. A later maintenance command can backfill candles from
existing raw rows, but startup should not run a large backfill synchronously.

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
- Candle tests proving OHLCV rows are updated from committed raw price deltas,
  sparse buckets are omitted, and candle history survives restart.
- Load tests proving large history ranges are paginated and do not allocate
  unbounded vectors.

## Linear Implementation Split

- SYB-146: persist sealed block history and exact-height fallback.
- SYB-147: replay WebSocket block stream from durable history.
- SYB-148: persist paginated market price history with retention policy.
- SYB-156: keep `frontend/DATA_MAP.md` aligned as these contracts change.
- SYB-160: add history retention metadata and bounded pruning.
- SYB-161: add price candle storage and API for long-window charts.

## Related Notes

- [[Persistence]] - recovery state and current volatile caches
- [[Block Data Boundaries]] - canonical blocks versus analytics sidecars
- [[WebSocket Block Stream]] - replay behavior
- [[REST API]] - endpoint surface
