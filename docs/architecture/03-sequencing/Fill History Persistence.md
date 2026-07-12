---
tags: [infrastructure, storage]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-07-12
---

# Fill History Persistence

`GET /v1/accounts/{id}/fills` is a user-facing history endpoint. It should survive restarts for the same reason balances and resting orders do: after a trade happens, the account owner expects the audit trail, portfolio UI, and bot status dashboard to remain consistent.

## Data Model

The sequencer already derives `AccountFillRecord` when finalizing a block. Each record stores:

- `order_id`
- `fill_qty`
- `fill_price`
- `block_height`
- `timestamp_ms`
- `position_deltas`: `(market_id, outcome, signed_delta)`

Persistence stores these records in redb table `fill_history`:

```text
key   = account_id || block_height || order_id
value = msgpack(AccountFillRecord)
```

All key components are big-endian `u64`, so lexicographic order groups records
by account and then by block height. `save_block()` inserts the untrimmed delta
produced by the committing block; post-commit callers may contribute only cache
rows whose height equals that block. It never replays older hot-cache rows,
because that would resurrect rows removed by retention.

The timestamp-first `fill_history_by_time` index is committed atomically with
the primary row. It supports global age and row-ceiling eviction without an
all-table scan.

The public cursor is the stable string `"<block_height>.<order_id>"`, returned
on each fill as `cursor`. `GET /v1/accounts/{id}/fills?after=<cursor>&limit=N`
returns matching fills strictly after that cursor in ascending cursor order.
`0.0` is the start sentinel. The older `offset` query remains deprecated. The
response envelope includes `fills`, `next_after`,
`retention_min_timestamp_ms`, `pruned_through_height`, `cursor_gap`, and
`history_truncated`; start means all retained history, not an indefinite
archive. Bot clients stop and reconcile canonical portfolio state when a saved
forward cursor falls at or below the pruned-through height.

## Recovery

Startup reads retained `fill_history` rows, decodes the account id from the key,
and hydrates `FillRecorder` before the actor starts serving traffic. Store-backed
deployments serve directly from redb; durable read failures are errors, never
fake empty hot-cache responses.

Market filtering remains in memory for now by checking each record's `position_deltas`. That is acceptable at current scale and keeps the first implementation simple. If fill history grows enough to make scans expensive, add a second index keyed by `(account_id, market_id, block_height, order_id)` rather than changing the API.

## Transaction Boundary

Fill history is committed inside the same redb transaction as the block header, market metadata, clearing prices, order-book snapshot, and account-state fence flip. The durable rows are sourced from an untrimmed per-block delta captured before the hot cache applies `SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT`, so a cap of `0` disables only the hot window and never suppresses durable rows. That gives a simple guarantee:

- If a block is committed, its fill history is committed.
- If a block is not committed, its fill history is ignored with the rest of the uncommitted block.

The table is derived, not validity-critical. It must match committed blocks, but account balances and positions remain authoritative in qmdb/redb state.

## Non-Goals

- Reconstructing history from old block witnesses on startup.
- Persisting price history or SSE replay buffers.
- Adding a market-level fill index before there is evidence the account-level scan is too slow.
- Treating derived fill rows as canonical balances or an indefinite audit archive.

## See Also

- [[Persistence]] — store layout and recovery rules
- [[Settlement]] — where fills mutate account state
- [[REST API]] — account fill endpoint
