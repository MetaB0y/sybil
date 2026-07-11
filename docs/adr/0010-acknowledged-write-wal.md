---
adr: 0010
title: Single-sequenced acknowledged-write WAL (durable-before-live)
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0010 — Single-sequenced acknowledged-write WAL (durable-before-live)

## Context

When the API returns **200 OK** for an order, cancel, deposit, or key-op, the
user is entitled to treat it as accepted — it must survive a crash before the
next block commits. But the *effect* of that action (admitting the order into the
open batch, mutating live in-memory state) also has to happen for the exchange to
function. If the live mutation happens *before* the action is durable, a crash
between the two loses an acknowledged write — the exchange forgets something it
told a user it accepted.

## Decision

**Durable-before-live**, single-sequenced through the actor. Each mutating
action, before it touches live state:

1. Validates (often against a clone, so a rejection changes nothing).
2. Burns its replay nonce.
3. **Appends a control-plane / admit-log WAL row** (durable).
4. Only then applies the mutation to live state.

The admit path (`admit_or_defer`) rolls back the live effect if the WAL append
fails — the WAL row is the 200-OK contract. All of this is single-sequenced by
the ractor actor (one writer), and the WAL is *cleared inside the same redb block
commit transaction* that persists the block ([ADR-0002](0002-qmdb-state-single-commit-fence.md)),
so on restart the WAL replays exactly the acknowledged-but-not-yet-committed
actions, in order, before bridge WALs.

Source: vault note `docs/architecture/Acknowledged-Write WAL Replay.md` (self-
described "single-sequenced-WAL decision record"); the ordering flags in
the [historical decomposition review](https://github.com/MetaB0y/sybil/blob/main/design/archive/review-2026-07-02/god-module-decomposition.md) §3 (actor `admit_or_defer`,
`on_tick_inner`, WAL-before-apply).

## Alternatives considered

- **Live-first, persist-later (write-behind).** Rejected: the crash window loses
  acknowledged writes — unacceptable for a financial API's 200 OK.
- **Synchronous per-action fsync of full state.** Rejected: far too slow on the
  hot path; the WAL append is the cheap durable unit, and the full state is
  fenced once per block.
- **Multiple concurrent writers with locking.** Rejected: the single-actor
  sequencing is what makes the ordering guarantees (nonce burn, WAL order,
  commit) *simple to reason about* — it's the "actors over mutexes" convention.

## Consequences

**Good:** a 200 OK is a real durability promise; recovery is a deterministic
ordered WAL replay; the single-writer discipline makes the durable-before-live
ordering auditable in one place.

**Costs / constraints:** the ordering in `admit_or_defer` / `on_tick_inner` /
each handler is **load-bearing and must not be reordered** — the god-module
decomposition explicitly flags these as move-verbatim-only 🔴; the single actor
is a **throughput ceiling** (one writer) and a design constraint on any future
horizontal-scaling direction; the WAL-clear must stay *inside* the block commit
txn or a crash could double-replay.

**Follow-ups:** the actor decomposition preserves these orderings
(SYB-232 Phase 1); mailbox back-pressure monitoring exists for the single-writer
bottleneck (`docs/architecture/Actor Mailbox Monitoring.md`).
