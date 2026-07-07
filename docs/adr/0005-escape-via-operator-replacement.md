---
adr: 0005
title: Escape via operator replacement; L1 recovers cash only
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0005 — Escape via operator replacement; L1 recovers cash only

> **Refined by [ADR-0013](0013-exit-and-escape-model.md) (2026-07-07):** escape
> now *values open positions at the last batch clearing price* (paid as cash),
> not cash-only. Operator replacement remains the path to *resume trading*.
> Read "cash only" below as superseded on that point.

## Context

Sybil is an off-chain sequencer with an on-chain vault holding user collateral.
The credibility question every such system must answer: **if the operator
vanishes or turns malicious, how do users get their value out?** There are two
distinct things a user might want back: their **cash** (collateral balance) and
their **open positions** (in-flight prediction-market exposure). Forcing
positions to settle on L1 would mean re-implementing the entire matching/pricing
engine — and an oracle for every market's resolution — inside the vault
contract. That is enormous, and it would let a griefer force-close live markets.

## Decision

Two separate recovery paths, deliberately asymmetric:

- **Positions are recovered by operator replacement (R-A).** The state is fully
  reconstructable from the DA-published witnesses (or the two-file custody
  snapshot), so a *replacement operator* can be stood up on the last accepted
  root and continue running the exchange. Positions are never force-settled on
  L1.
- **Cash escapes on L1, conservatively.** The escape-claim guest (SYB-32) proves
  a single account-level fact against an accepted root —
  `withdrawable_cash = max(0, balance − open_cash_reservations)` bound to a
  recipient — and the vault pays that out. Cash-only, one claim per account per
  root, no position unwinding.

Sources: `design/escape-hatch-reconstruction.md` (SYB-80),
`docs/architecture/Operator Replacement.md` (SYB-116),
`design/escape-claim-guest.md` (SYB-32), vault note
`docs/architecture/L1 Settlement and Vault.md`.

## Alternatives considered

- **Full L1 force-settlement of positions.** Rejected: requires the pricing
  engine + a resolution oracle on-chain; a huge contract surface; and it hands a
  griefer a way to force-close markets that are still live and fairly priced.
- **DA-only escape (no leaf-proof path).** Rejected as the *sole* path: it makes
  data availability a single point of failure. The escape-claim guest therefore
  supports a **leaf-proof (Form L)** path that works from the user-held custody
  snapshot with zero DA dependency — an escape path that needs the operator's
  infrastructure isn't an escape path.

## Consequences

**Good:** the vault stays small and auditable (verify a proof, pay cash); users
have a trustless cash floor that does not depend on the operator being alive;
positions are preserved (not liquidated) through operator handoff.

**Costs / constraints:** escape-claim requires the account leaf to **commit to
the account's signing keys** — today it does not, which is the hard prerequisite
driving [ADR-0008](0008-in-guest-p256-openvm-ecc.md) and SYB-225 (`keys_digest`);
`escapeClaim` must **bypass the pause flag** (escape exists for when governance
is the problem) — a carve-out from the SYB-96 single `paused` flag; and the
"newest-accepted-root-only" freshness rule is what keeps escape from
double-paying an already-finalized withdrawal (see the ratification packet D8/D9).

**Follow-ups:** `keys_digest` in account leaves (SYB-225), the escape-claim guest
+ vault entrypoint (SYB-32), the custody-snapshot CLI (R1).
