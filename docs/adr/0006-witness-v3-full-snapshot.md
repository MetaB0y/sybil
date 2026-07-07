---
adr: 0006
title: Block witness = full-state snapshot per block
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0006 — Block witness = full-state snapshot per block

> **Refined by [ADR-0012](0012-privacy-and-data-availability.md) (2026-07-07):**
> the full snapshot is a **private prover input, NOT a public DA payload**. Only
> the root + proof + opaque hashes are public; leaf contents stay private. Read
> "published to DA" below as "consumed privately by the prover." Also, per
> [ADR-0011](0011-validium-stance-no-backcompat.md) the version bump is *free*
> (fresh genesis), and v-structs should be **flat**, not `V4{base:V3}`.

## Context

Each block emits a **witness**: the data the verifier/guest consumes to check
the state transition, and the data published to DA so the state is
reconstructable. A witness can be *incremental* (a diff against the prior state
that only makes sense in a chain) or *self-contained* (everything needed to
reconstruct the post-state from that one payload). This choice governs
reconstruction complexity, the escape path, and how forgiving the system is of a
missing historical payload.

## Decision

The canonical witness (currently **v3**) is a **full-state snapshot per block**:
one payload carries the pre-state, system events, fills, sidecars, and post-state
sufficient to **fully reconstruct the block's state independently**. Reconstruct
from a single accepted payload; you do not need to replay the whole chain.
`decode_canonical_witness_bytes` is the canonical decoder (SYB-227), and the
witness bytes feed `witness_root` → DA commitment → the guest public input.

Sources: `docs/architecture/Block Witness.md` (v3 as-landed),
`design/witness-schema-v2.md`, and the v3 update in commit `397fabe2`.

## Alternatives considered

- **Incremental/diff witnesses.** Smaller payloads, but reconstruction requires
  the entire prior chain and every intermediate payload — a single missing DA
  blob poisons everything downstream. Rejected: it makes the escape/custody path
  fragile and reconstruction stateful.
- **Snapshot only at checkpoints, diffs between.** Rejected for now as premature
  optimization; the payload-size cost of full snapshots is acceptable at current
  scale and the reconstruction simplicity is worth far more than the bytes.

## Consequences

**Good:** reconstruction is a pure function of one payload (the escape-claim
"Form P" path and the custody CLI both rely on this); a missing historical
payload doesn't cascade; the decoder can enforce canonical byte order by
re-encoding what it decodes (a strong anti-malleability property).

**Costs / constraints:** payloads are **larger** (full state each block) — a real
DA-cost and bandwidth consideration as state grows, and the eventual reason a
validium/DA-tiering direction exists (`docs/architecture/Data Availability.md`);
**any change to state or witness fields is a canonical-format version bump**
(v3→v4) that moves the state root and the guest commitment and forces a fresh
genesis in devnet ([ADR-0009](0009-fresh-genesis-for-consensus-changes.md)); the
`keys_digest` addition (SYB-225) is exactly such a v3→v4 bump.

**Follow-ups:** put `witness_root` into the block header for anti-equivocation
(`design/general-advice-2026-07.md` open items); revisit checkpoint+diff if DA
cost dominates at scale.
