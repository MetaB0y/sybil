---
adr: 0009
title: Fresh genesis for consensus-schema changes in devnet
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0009 — Fresh genesis for consensus-schema changes in devnet

> **Relaxed by [ADR-0011](0011-validium-stance-no-backcompat.md) (2026-07-07):**
> fresh genesis has **zero cost** (not live; no backward-compat until autumn), so
> drop the "batch changes into one redeploy window" framing — just change the
> schema and restart. "consensus-schema" here means **validity-schema** (single-
> operator validium).

## Context

Changes to the account-leaf schema, the canonical witness format, or the guest
program **move the state root and/or the guest commitment**. Old accepted roots
were produced under the old schema; old witnesses decode under the old version.
A running chain cannot simply "switch" to a new schema mid-stream without either
a migration block that all verifiers treat as a hard version boundary, or a
re-genesis. While the project is in **early devnet** (no real user funds to
preserve across the boundary), the cost/benefit of a live migration is
lopsided.

## Decision

For validity-schema changes in devnet, **recreate genesis** rather than migrate
in place. The procedure: implement the new commitments/guest, recreate genesis
from an operator-controlled snapshot (including, for SYB-225, full active
key-sets so every funded account has ≥1 key), **repin the OpenVM guest
commitment**, and deploy against the new genesis/root series. All fingerprint
work runs in `~/sybil` per the SYB-228 repin discipline (`just openvm-commit`,
refresh the `OpenVmVerifierAdapter.sol` pin, `zk-guest-fingerprint.sh --write`,
no fmt between write and check). Batch multiple validity changes into one
fresh-genesis window (SYB-224 + SYB-225 + the P-256 extension together).

Precedent: `design/archive/implemented/witness-schema-v2.md` (canonical-witness changes force a
guest repin + fresh genesis in devnet); runbook
`docs/runbooks/fresh-genesis-redeploy.md`.

The lightweight deployment-boundary gate records this decision per exact
validity-artifact fingerprint in `deploy/validity-boundary.json`. Canonical
vectors, desired pins, guest fingerprints/commitments, or resolved Commonware
changes cannot pass `just check-consensus` until the new fingerprint explicitly
declares `fresh_genesis` (this ADR/runbook) or references a reviewed migration
plan. The record requires the decision; it does not claim deployment occurred.

## Alternatives considered

- **In-place migration block** (freeze writes, rewrite every account leaf,
  produce a synthetic version-boundary root). Retained as the **documented
  fallback for mainnet**, where re-genesis would destroy real state — but
  rejected for devnet as unnecessary complexity now.
- **Backward-compatible schema (serde defaults, optional fields).** Rejected as
  the general strategy: it accretes transitional cruft in the *validity*
  encoders, and a "defaulted" field still changes the bytes and the root. Fine
  for host-side DTOs, not for the state leaf.

## Consequences

**Good:** validity changes stay clean — no migration machinery, no transitional
compatibility shims in the byte encoders, one hard version boundary; batching
amortizes the redeploy cost.

**Costs / constraints:** every validity change is gated on a redeploy, so they
**must be batched and scheduled**, not shipped piecemeal; devnet state is
**not preserved** across the boundary (acceptable now, not forever); the mainnet
migration path is *documented but unexercised* — it will need a real drill before
mainnet (ties to the escape/restore drill culture, SYB-223). This ADR is why the
D-cluster (SYB-224/225/32/P-256) is sequenced into a single fresh-genesis
redeploy.

**Follow-ups:** exercise the in-place migration fallback on a throwaway
deployment before mainnet; keep `fresh-genesis-redeploy.md` turnkey (current final-tag
pins: exe `0x000f896e`, vm `0x007a02fc`).
