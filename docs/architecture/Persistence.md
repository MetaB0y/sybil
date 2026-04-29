---
tags: [infrastructure, storage]
layer: sequencer
status: current
last_verified: 2026-04-29
---

# Persistence

Sybil persists exchange state to survive crashes without losing accounts, markets, or positions.

## Philosophy

**Block-boundary snapshots, not event sourcing.** Each block is the transactional unit. After a block is prepared and accepted for commit, we persist the committed state and resume from that point on restart. Anything in-flight at crash time (mempool contents, current solve, transient actor state) is discarded and rebuilt by normal client behavior.

This follows [[Block Lifecycle]]: the block is the natural checkpoint. We do not replay a long event log at startup and we do not attempt to preserve partial progress inside a block.

## Storage Split: redb + qmdb

Sybil currently uses two storage engines with distinct authority boundaries:

- **qmdb** stores account snapshots plus the typed v2 leaves currently hashed
  into the block header state root.
- **redb** stores block metadata, market data, counters, and the authoritative commit fence that says which qmdb snapshot is committed.

This is intentionally not "one transaction across two databases". There is no journal and no cross-db transaction. Instead, redb is the only commit point:

1. Write the next account snapshot and typed v2 leaves into the inactive qmdb
   slot.
2. Commit the redb transaction that stores the new block metadata and flips the authoritative fence to that slot.
3. Recover strictly from the fence recorded in redb.

Anything written to qmdb without a corresponding redb fence flip is treated as uncommitted and ignored.

### Why This Split Makes Sense Today

- **qmdb fit**: accounts are the large authenticated keyspace and the part most likely to evolve toward proof-oriented storage.
- **redb fit**: metadata, counters, block headers, and fence coordination remain simple inside one ACID write transaction.
- **Simple crash model**: one authoritative fence, one authoritative slot, no ambiguity about "latest".
- **Honest semantics**: we do not pretend to have cross-db atomicity that the system does not actually provide.

**Serialization**: structured values are stored with `rmp-serde` MessagePack. That keeps values self-describing and tolerant of additive schema changes via `#[serde(default)]`.

### Commitment Direction

This split is the current runtime persistence model, not the final
cryptographic commitment model. [[State Root Schema]] now hashes typed v2
leaves for accounts, bridge state, active resting orders, and aggregate
reservations, and those exact leaves are persisted in qmdb under the fenced
slot. The public root is still a canonical SHA-256 digest over sorted
key/value leaves, not qmdb's native MMR root. The native qmdb target promotes
qmdb from storage to the authenticated root/proof system over accounts,
reservations, resting orders, market lifecycle state, and system counters.
redb can continue to own simple metadata and the crash-recovery fence.

## Tier 1: Core State

Authoritative state needed to resume the exchange after a crash:

| Engine | Namespace / Table | Role |
|--------|--------------------|------|
| `qmdb` | slot-prefixed account snapshot keys | `Account` rows plus slot-local `height` and `next_account_id` |
| `qmdb` | slot-prefixed `v2:` typed leaf keys | canonical account, bridge, order, and reservation leaves committed by `state_root_v2` |
| `redb` | `markets` | market definitions |
| `redb` | `market_meta` | market metadata |
| `redb` | `market_statuses` | market status driven by oracle/system logic |
| `redb` | `market_groups` | market groups |
| `redb` | `block_headers` | canonical block header by height |
| `redb` | `pubkey_registry` | compressed pubkey to account id |
| `redb` | `clearing_prices` | last clearing price vector per market |
| `redb` | `market_volumes` | cumulative traded volume per market |
| `redb` | `counters` | next IDs, store layout version, and the authoritative account-state fence |

The account snapshot uses two logical qmdb slots, `A` and `B`. Only one slot is committed at a time; redb records which one.

## Tier 2: Order State

Persisted today:

- **Order book**: resting orders and their reservations in `resting_orders`
- **Deferred submissions**: MM / bundle / multi-market submissions in `pending_bundles`
- **Admit log**: direct-admitted single-market orders accepted after the last committed block in `admit_log`

Still not persisted:

- **MM runtime state**: inventory, short-term price history, and variance estimation state

The order state tables protect the API's 200 OK contract: anything acknowledged before a crash is either already in the committed order-book snapshot or replayed from an incremental log.

## Tier 3: Derived Views

Persisted today:

- **Fill history**: per-account fill records in `fill_history`, keyed by `(account_id, block_height, order_id)`. See [[Fill History Persistence]].

Still not persisted:

- **Price history**
- **Block ring buffer for SSE catch-up**
- **Derived aggregates such as welfare summaries**

These are reconstructable or refreshable, but expensive or inconvenient to rebuild.

## Recovery Order

Startup recovery is intentionally fence-driven:

1. Open redb and validate `store_layout_version`.
2. Read the canonical block height and canonical account-state fence from redb.
3. Read only the fenced qmdb slot.
4. Reject the store as corrupt if the fenced qmdb slot's `height` or `next_account_id` does not match redb.
5. Restore the in-memory sequencer state from those committed structures.

Recovery never scans qmdb looking for "the newest" snapshot. The fence is the authority.

## Invariants

The current model relies on explicit invariants:

- `store_layout_version` must exist and match the binary's expected layout.
- If `height` exists, then `account_state_height` and `account_state_slot` must also exist.
- `height == account_state_height`.
- The qmdb slot named by `account_state_slot` must contain matching `height` and `next_account_id`.
- Recovery trusts redb's fence, not qmdb recency.

When any of these fail, startup should reject the store as unsupported or corrupt rather than guessing.

## Crash Cases

- **Crash before qmdb finishes writing the inactive slot**: redb fence is unchanged, so recovery uses the previous committed slot.
- **Crash after qmdb finishes but before redb commits**: the new qmdb snapshot exists but is ignored as uncommitted.
- **Crash after redb commits**: the new slot is authoritative and recovery uses it.

This is the whole reason the commit fence lives in redb.

## Preserved vs Lost

**Preserved on crash:**

- All account balances and positions
- All markets, groups, metadata, and statuses
- Block headers
- Pubkey registry
- Counter state and next IDs
- Resting orders and reservations
- Direct-admit recovery log and deferred submissions admitted after the last committed block
- Fill history

**Lost on crash:**

- Price history cache
- SSE ring buffer contents
- Transient external feed state such as recently pushed reference prices

## How to Add New Persisted State

1. Decide which store is authoritative for the new state.
2. If it is metadata, counters, or coordination state, prefer redb.
3. If it is large authenticated per-key state, consider qmdb.
4. Add serialization in `save_block()` and deserialization in `load_state()`.
5. If the new state participates in crash recovery, document its invariants explicitly.
6. Wire it into `BlockSequencer::restore()` if needed.

Do not add state to both stores unless there is a clear authority boundary and recovery rule.

## Files

| File | Role |
|------|------|
| `crates/matching-sequencer/src/store.rs` | redb metadata store, layout checks, authoritative commit fence |
| `crates/matching-sequencer/src/account_storage.rs` | account snapshot boundary and fenced recovery contract |
| `crates/matching-sequencer/src/qmdb_accounts.rs` | qmdb-backed account snapshot implementation |
| `crates/sybil-api/src/main.rs` | opens store and restores or bootstraps state |
| `crates/sybil-api/src/config.rs` | `--data-dir` / `SYBIL_DATA_DIR` config |

## Related Notes

- [[Block Lifecycle]] — the transactional unit we persist at
- [[Settlement]] — what mutates committed state
- [[State Root and Parent Hash]] — integrity verification of committed state
