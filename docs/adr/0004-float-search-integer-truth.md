---
adr: 0004
title: Float-search, integer-truth
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0004 — Float-search, integer-truth

## Context

The batch clearing ([ADR-0001](0001-eg-fisher-market-matching.md)) is a convex
program solved by numerical solvers (HiGHS LP, SLP, Clarabel) that work in
**floating point**. But the state transition is **validity-critical** — balances,
positions, and the state root must be **exactly reproducible** on every node and
inside the guest, where floating-point non-determinism is unacceptable and
`f64` rounding would make two honest nodes disagree on the last ulp.

## Decision

Split the pipeline into a **float search** and an **integer truth**:

- Solvers may use `f64` freely to *find* an approximate clearing (prices,
  fill quantities).
- Everything that enters state is then **quantized to integers** ("nanos",
  fixed-point) and all settlement/balance/root arithmetic is
  **exact overflow-checked integer** math. `sybil-verifier` re-derives the
  outcome with integer arithmetic and is the source of truth; the solver's
  floats are a *proposal*, never the record.

Conventions: `docs/architecture/Nanos and Integer Arithmetic.md`, the
"integer truth" framing in `design/architecture-review-2026-07.md` §1, and the
AGENTS.md all-integer convention.

## Alternatives considered

- **Exact/rational arithmetic in the solver.** Rejected: convex solvers are
  float-native; a rational LP would be orders of magnitude slower on the hot
  path for no gain, since we re-verify in integers anyway.
- **Trust the solver's floats as state.** Rejected: non-deterministic across
  platforms and the guest; a single differing rounding breaks validity.
- **Fixed-point *inside* the solver.** Rejected: fights the numerical libraries;
  the clean seam is "float proposes, integer disposes."

## Consequences

**Good:** solvers stay fast and swappable; the proven core stays exactly reproducible;
the verifier is a genuine independent check on the solver rather than a rubber
stamp.

**Costs / constraints:** the **quantization boundary is a real place bugs live** —
a fill that is a few nanos over a limit price in `f64` but exact-rejected by the
integer verifier is a known special case (historical [cross-cutting review](https://github.com/MetaB0y/sybil/blob/main/design/archive/review-2026-07-02/02-cross-cutting-themes.md)
Theme 3 / P2); the solver and verifier must agree on the *rounding rule*, not
just the arithmetic; and "all-integer" is a convention the codebase has violated
in spots (stated in prose, not enforced by types — the newtype-`Nanos`/`Qty`
the historical [audit roadmap](https://github.com/MetaB0y/sybil/blob/main/design/archive/review-2026-07-02/30-roadmap.md) Phase 3 proposed making it a type-level
constraint).

**Follow-ups:** `Nanos`/`Qty` newtypes + workspace lints to make the boundary
un-crossable by accident.
