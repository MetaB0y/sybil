---
tags: [infrastructure, storage, recovery]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-07-14
---

# Acknowledged-write WAL replay

Between block snapshots, every state-affecting command already accepted by the
sequencer actor is stored in one globally sequenced redb write-ahead log. On
restart, the complete interval is replayed in exact actor acceptance order on
top of the last fenced snapshot. Recovery refuses to start from a partial,
reordered, undecodable, or semantically invalid interval.

This is the acknowledged-write half of [[Persistence]]. The redb/qMDB block
fence remains the committed-state authority; WAL rows are only the durable
between-block suffix that the next normal block folds into a new snapshot.

## Why one global sequence is required

The previous layout used separate tables for direct admits, deferred bundles,
control-plane commands, deposits, withdrawal creation, and L1 lifecycle input.
It replayed classes in a fixed order. That was sufficient while the
cross-subsystem interleaving did not enter a validity-sensitive artifact.

Ordinary order/cancel authorization changes that boundary. A signed action
advances an account replay nonce, key operations can change the valid signer
set, and accepted orders/cancels must be witnessed in the same order the actor
accepted them. Reconstructing a different cross-table order can therefore
change action validity or witness bytes. The conditional migration trigger in
the prior design has fired.

The July 1 cancel/withdrawal incident was the earlier warning: both rows were
durable, but replaying withdrawal creation before the cancellation released its
reservation could reject an acknowledged withdrawal. One global sequence
removes the whole class of hidden pairwise ordering rules.

## Format

`ACKNOWLEDGED_WRITES` is keyed by a lifetime-monotonic `u64` sequence. Each
value is named MessagePack for a versioned envelope:

```text
AcknowledgedWriteEnvelope {
    version,
    sequence,       // must equal the redb key
    write,
}

AcknowledgedWrite =
    DirectAdmit(RestingOrder)
  | DeferredBundle(OrderSubmission)
  | ControlPlane(ControlPlaneCommand)
  | L1Deposit(L1Deposit)
  | BridgeWithdrawal(BridgeWithdrawalRequest)
  | BridgeL1Input(BridgeL1Input)
```

The variant enum is append-only within an envelope version. Breaking changes
require a new envelope/store layout under the fresh-genesis policy.

Two redb counters authenticate the pending interval:

- `acknowledged_write_floor`: first sequence not included in the committed
  block snapshot;
- `next_acknowledged_write_seq`: sequence to allocate to the next accepted
  write.

The table must contain exactly every key in `[floor, next)`. This detects loss
of the first row, an interior gap, a stale row below the floor, and a row at or
above `next`. The repeated sequence inside the value detects moving otherwise
valid bytes to a different key.

## Append and acknowledgement

The sequencer actor serializes mutation. Each append allocates the sequence,
writes the versioned envelope, and advances `next` in one redb transaction.
The caller returns success only after that transaction commits.

Existing live-ordering disciplines remain explicit:

- control-plane, deferred, deposit, withdrawal, and L1-input commands persist
  before mutating live state;
- a direct resting-order admit is applied live first, then durably appended;
  append failure rolls the admit back before returning an error;
- no acknowledged write is allowed before the first committed block snapshot,
  because there would be no recovery baseline on which to replay it.

Fresh persistent API startup therefore commits a baseline block before
bootstrap commands or HTTP traffic can be accepted.

## Replay

Recovery performs these steps:

1. load only the qMDB slot named by the redb commit fence;
2. restore the committed redb snapshot and resting-order book;
3. repair-expire any stale order already committed through the snapshot height;
4. validate and decode the exact `[floor, next)` acknowledged-write interval;
5. dispatch rows in ascending sequence through the normal deterministic
   mutation handlers;
6. advance `next_order_id` while replaying both direct admits and deferred
   bundles; and
7. rebuild derived indexes only after the whole interval succeeds.

`DeferredBundle` retains its normal meaning: replay places it in the pending
queue and the next block revalidates it. It does not become an inline committed
transition. Direct admits restore their already-validated reservation rows.

Any application error stops recovery. An acknowledged row is never dropped
and counted as benign; running from a partial prefix would make the recovered
state disagree with what clients were told succeeded.

## Block-fence interaction

`save_block_inner` writes the new snapshot and, in the same redb transaction
that flips the qMDB fence:

1. clears `ACKNOWLEDGED_WRITES`; and
2. sets `acknowledged_write_floor = next_acknowledged_write_seq`.

The lifetime sequence is not reset. A crash before this transaction leaves the
old fence and complete WAL suffix authoritative. A crash after it leaves the
new fence, empty interval, and advanced floor authoritative.

## Observability

The store exports:

- `sybil_acknowledged_writes_appended_total{kind}`;
- `sybil_acknowledged_write_committed_floor`;
- `sybil_acknowledged_write_next_sequence`;
- `sybil_acknowledged_write_pending_rows`; and
- `sybil_restore_acknowledged_write_failures_total{kind}`.

Structural/envelope failures use `kind="stored_log"`; deterministic replay
failures use the acknowledged-write variant kind.

Any restore failure is an integrity incident. Preserve the store, stop writes,
and investigate the missing/corrupt row or deterministic replay divergence.

## Compatibility

This change is store layout v2 and intentionally does not infer a fake global
order from the old per-table layout. The project is pre-launch and the
authorization/witness migration already requires a fresh genesis and guest
repin, so old v1 stores are rejected rather than migrated with invented order.

## Tests

Load-bearing coverage includes:

- mixed control-plane/direct/deferred/deposit/withdrawal/L1-input sequence
  preservation;
- first-row gap detection using the committed floor;
- lifetime sequence continuity across block fences;
- rejection before a committed replay baseline;
- repeated actor restart before the next block;
- cancel before withdrawal replay;
- L1 cancellation refund idempotence;
- order-id advancement across direct and deferred rows; and
- fail-closed semantic rejection of invalid acknowledged rows.

## Related notes

- [[Persistence]]
- [[Block Lifecycle]]
- [[Order Admission]]
- [[Pending Orders and TTL]]
- [[State Root Schema]]
- [[Testing Strategy]]
