---
tags: [infrastructure, storage]
layer: sequencer
status: active
last_verified: 2026-04-10
---

# Persistence

Sybil persists exchange state to survive crashes without losing accounts, markets, or positions.

## Philosophy

**Block-boundary snapshots, not event sourcing.** Each block is an atomic unit (orders → solve → fills → settlement → new state). After each block, we write the complete state to disk in a single ACID transaction. On crash, we load the last committed state and resume. Anything in-flight (mempool, current solve) is lost — clients resubmit within seconds.

This is inspired by [[Block Lifecycle]] — the block is the natural transactional boundary. See also absurd's "step-based checkpointing" pattern: once a step completes, its result is persisted and never re-executes.

## Storage: redb

We use [redb](https://github.com/cberner/redb) — a pure-Rust embedded key-value store with ACID transactions and crash recovery.

**Why redb over alternatives:**
- **Simplest API**: `table.insert(key, value)` maps directly to our HashMaps
- **ACID**: One write transaction per block = one fsync per 2 seconds
- **Crash recovery**: Rolls back to last committed transaction on unclean shutdown
- **Pure Rust**: No C dependencies (unlike SQLite)
- **Data model fit**: Our state is key-value, not relational

**Serialization**: MessagePack via rmp-serde. Self-describing and binary-stable across schema changes — adding fields with `#[serde(default)]` is backward-compatible. No migration code needed.

## Three Tiers

### Tier 1: Core State (implemented)

Authoritative state needed to resume the exchange after crash:

| Table | Key | Value |
|-------|-----|-------|
| `accounts` | AccountId (u64) | Account (balance, positions, total_deposited, events_digest) |
| `markets` | MarketId (u32) | Market (name) |
| `market_meta` | MarketId (u32) | MarketMetadata (description, tags, status) |
| `market_statuses` | MarketId (u32) | MarketStatus (Active, Resolved, etc.) |
| `market_groups` | group_idx (u32) | MarketGroup (name, market_ids) |
| `block_headers` | height (u64) | BlockHeader (hash, state_root, counts, timestamp) |
| `pubkey_registry` | compressed P256 (33 bytes) | AccountId (u64) |
| `clearing_prices` | MarketId (u32) | Vec\<Nanos\> (last clearing prices per market) |
| `counters` | name (&str) | value (u64) |

Written in a single transaction after each block (~70KB, 2-second interval).

### Tier 2: Order State (TODO)

State for seamless order continuity across restarts:

- **Order book**: The `OrderBook` component owns all resting orders and their balance/position reservations. Persisting it preserves resting orders + committed capital across restarts. Natural serialization boundary — one struct, one table.
- **Mempool**: In-flight submissions not yet in a block. Low priority (clients resubmit).
- **MM state**: Per-market inventory, price history for variance estimation.

### Tier 3: Derived Views (TODO)

Queryable history reconstructable from blocks, but expensive to rebuild:

- **Fill history**: Per-account fill records (FillRecorder)
- **Price history**: Per-market clearing price timeseries (PriceTracker)
- **Block ring buffer**: Last 100 full blocks for SSE catch-up
  Full blocks now include `system_events` alongside fills/rejections.
- **Volume/welfare aggregates**

## Crash Recovery Semantics

**Preserved on crash:**
- All account balances and positions
- All markets, groups, metadata, statuses
- Block chain (headers)
- P256 pubkey registry
- Counter state (next IDs)

**Lost on crash:**
- Mempool (pending submissions) — clients resubmit
- Pending orders (TTL 3 blocks = 6 seconds) — acceptable loss
- Fill/price history — Tier 3 will fix this
- Block ring buffer — SSE subscribers reconnect
- Reference prices — Polymarket mirror re-pushes within seconds

## How to Add New Persisted State

1. Define a new `TableDefinition` in `store.rs`
2. Add serialization in `save_block()` (inside the write transaction)
3. Add deserialization in `load_state()` (to `RestoredState`)
4. Wire into `BlockSequencer::restore()` if needed
5. Add `#[derive(Serialize, Deserialize)]` to any new types

## Files

| File | Role |
|------|------|
| `crates/matching-sequencer/src/store.rs` | Store struct, save/load, table definitions |
| `crates/sybil-api/src/main.rs` | Opens store, loads or creates fresh state |
| `crates/sybil-api/src/config.rs` | `--data-dir` / `SYBIL_DATA_DIR` config |

## Related Notes

- [[Block Lifecycle]] — the transactional unit we persist at
- [[Settlement]] — what state changes each block
- [[State Root and Parent Hash]] — integrity verification of persisted state
