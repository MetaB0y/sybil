---
adr: 0002
title: QMDB authenticated state behind a single redb commit fence
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0002 — QMDB authenticated state behind a single redb commit fence

## Context

The sequencer needs two things from its state store that are in tension:

1. An **authenticated state root** — a cryptographic commitment over all account
   and market state that the verifier/guest can check and that L1 can anchor.
2. **Durable, crash-consistent persistence** — the operational database (order
   books, history, WALs, analytics, the state itself) with the guarantee that a
   block is either fully committed or not at all.

If these live in two independently-committed stores, a crash between their
commits leaves the authenticated root and the operational data disagreeing about
what the last block was — an unrecoverable split brain.

## Decision

Two stores, **one commit authority**:

- **QMDB** (`qmdb_state.rs` / `qmdb_accounts.rs`) produces the authenticated
  **state root** — a commonware ordered-current SHA-256 qMDB over the canonical
  state leaves. This root is what `sybil-verifier` recomputes and the guest
  proves.
- **redb** (`store.rs`) is the operational database. **Every block is written in
  a single redb write transaction** (`write_redb_block_commit_inner`), and the
  qMDB root is verified *before* that transaction's fence flip and `commit()`.
  There is exactly one place a block becomes durable.

The redb and QMDB paths are **not redundant** — QMDB authenticates, redb
persists — and the fence makes them agree by construction. Sources:
`docs/architecture/Persistence.md`, the fence discussion in
`design/architecture-review-2026-07.md` §1.

## Alternatives considered

- **Single store doing both (authenticate *and* persist).** Rejected: no
  off-the-shelf embedded KV gives both a cheap ordered Merkle root and redb's
  transactional ergonomics for the dozens of non-authenticated tables (history,
  WALs, analytics). Forcing everything through the Merkle DB would make every
  analytics write a state-root event.
- **Two independently-committed stores.** Rejected outright — the split-brain
  failure above.
- **Commit the operational store first, derive the root lazily.** Rejected: the
  root must be verified *before* the block is acknowledged, or we'd durably
  commit a block whose root we can't prove.

## Consequences

**Good:** crash consistency is a structural property, not a runtime check — you
cannot commit a block whose authenticated root wasn't verified first; recovery
(`load_state`) is a single fence-driven ordered replay; the two concerns
(authenticate vs persist) stay cleanly separated by responsibility.

**Costs / constraints:** the commit path (`write_redb_block_commit_inner`, ~30
tables in one txn) is **irreducibly large and must stay atomic** — it is
explicitly called out as "do not split the txn" in the god-module decomposition;
there is a hard "single commit authority" invariant that any refactor near
persistence must preserve; and the qMDB-before-fence ordering in `load_state`
recovery is load-bearing and fail-closed.

**Follow-ups:** the single-sequenced WAL that rides this fence is
[ADR-0010](0010-acknowledged-write-wal.md); `store.rs` decomposition keeps the
fence whole (`docs/review/god-module-decomposition.md` §2).
