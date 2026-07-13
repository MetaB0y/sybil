# `matching-sequencer`

Single-writer exchange state machine and persistence layer. `BlockSequencer` is
the deterministic synchronous core; a supervised `ractor` actor exposes it
through `SequencerHandle`. Simulation-only agents remain in `sequencer-sim`.

## Read first

- [[Block Lifecycle]] and [[Order Admission]]
- [[Settlement]] and [[Pending Orders and TTL]]
- [[Persistence]] and [[Acknowledged-Write WAL Replay]]
- [[Block Witness]] and [[State Root Schema]]

## Load-bearing rules

- Prepare a block on a clone; persist it and flip the redb commit fence before
  swapping live state or publishing.
- Every acknowledged between-block mutation is durable-before-live and follows
  the fixed WAL replay order.
- redb is the commit authority; recovery reads only the fenced qMDB slot.
- `OrderBook` owns resting-order reservations. Replay re-derives aggregates.
- Settlement uses shared `matching-engine` integer helpers and MINT derivation.
- Unsupported value-bearing order shapes fail closed at admission.
- The header state root covers typed account, bridge, market, group, lifecycle,
  resting-order/reservation, counter, and withdrawal/claim state—not merely an
  account-vector hash.
- Derived analytics/history are useful but never validity inputs.

## Block path

```text
actor mailbox → durable admission/WAL → prepare cloned transition
→ solve → integer settlement → block + witness → persist/fence
→ live-state swap → realtime publication
```

`BlockSequencer::try_produce_block` returns `BlockProduction`. Actor callers use
the persistence-aware prepare/persist/commit path and publish `SealedBlock`.

## Code map

| Area | Location |
|---|---|
| State transition | `sequencer.rs`, `sequencer/production/`, `sequencer/*` |
| Actor/RPC/supervision | `actor.rs`, `actor/`; handle domains in `actor/handle/` |
| Resting orders | `order_book.rs` |
| Shared settlement orchestration | `settlement.rs` |
| Canonical state/block | `canonical_state.rs`, `block.rs`, `system_event.rs` |
| Persistence, WAL, DA, import | `store.rs`, `store/` |
| Auth/signature checks | `crypto.rs` |
| Bridge and lifecycle | `bridge.rs`, `market_lifecycle.rs` |
| Off-block views | `aggregates/`, `analytics.rs`, trackers/recorders |

Defaults live in `sequencer/config.rs`; API/env overrides may differ. Do not
duplicate changing limits in documentation when a code pointer is clearer.

Run `cargo test -p matching-sequencer`; persistence changes also require crash,
restart, import, and API integration coverage.
