---
adr: 0001
title: Eisenberg–Gale / Fisher-market matching
status: Accepted
date: 2026-07-07
consensus_critical: true
supersedes: []
superseded_by: []
---

# ADR-0001 — Eisenberg–Gale / Fisher-market matching (not LMSR or a CLOB)

## Context

Sybil clears prediction markets in **frequent batch auctions**: orders that
arrive during a block are matched together at a single clearing, not
continuously. The central question is *how a batch is cleared* — what prices
come out, and how those prices stay coherent when a single order touches
multiple outcomes or multiple markets (bundles, spreads, conditionals on the
roadmap).

The clearing rule is **consensus-critical**: `Order`, `Fill`, and the settlement
math are serialized into the block witness and re-derived by `sybil-verifier` /
the guest, so the clearing semantics are baked into the state-transition proof.

## Decision

Clear each batch as an **Eisenberg–Gale convex program** — i.e. compute a
**Fisher-market competitive equilibrium**. Traders are buyers with budgets;
outcome shares are the goods; the EG program maximizes the budget-weighted sum
of log-utilities, and its **dual variables are the clearing prices**. Coherence
across composed markets is not imposed by side-constraints — it *emerges* from
solving one joint program. The production clearer is the **LP solver**
(`matching-solver`, `features=["lp"]`); EG/IterLP/Conic exist as
differential-testing oracles.

Rationale and the LMSR-equivalence proof: `design/lmsr-proof.typ`
("Prediction Markets Are Fisher Markets"), `design/decomposition.typ`,
`docs/architecture/Welfare Maximization.md`.

## Alternatives considered

- **LMSR (logarithmic market-scoring-rule AMM).** The standard prediction-market
  maker. Rejected as the *core*: it is path-dependent (price depends on trade
  order — antithetical to batch fairness), requires an explicit subsidy/loss
  bound, and does not yield the welfare-optimal joint clearing across bundles.
  We keep its *scoring intuition* (the proof shows the FBA clearing coincides
  with LMSR at equilibrium) but not its mechanism.
- **Central limit order book (CLOB).** Continuous matching. Rejected: it
  re-introduces the latency race the batch auction exists to kill, and it has no
  native notion of a *coherent* multi-outcome price — you'd bolt on
  cross-market constraints and hope they stay consistent.
- **Ad-hoc batch matching with coherence side-constraints.** Rejected: the
  constraints multiply with every new instrument type and are a bug farm; the EG
  formulation gets coherence for free.

## Consequences

**Good:** prices are welfare-optimal and internally coherent by construction;
within a batch there is no time priority, so no HFT/latency advantage;
multi-outcome and bundle pricing fall out of the same program; there is a clean
mathematical spec to verify against (the dual gives a checkable KKT witness).

**Costs / constraints:** every block must solve a convex program — the solver is
on the hot path and its cost bounds block cadence; the clearing is only as
trustworthy as the solver↔verifier conformance harness; and because `Order` and
the settlement math are in the guest commitment, **generalizing the instrument
(payoff vectors, conditionals) is a consensus change**, not a local feature
(see [ADR-0006](0006-witness-v3-full-snapshot.md), and the payoff-vector
generality deferred in `docs/review/30-roadmap.md`).

**Follow-ups:** combinatorial-markets direction (`design/bundle-clearing.typ`,
`design/decomposition.typ`); the float-search/integer-truth split this forces is
[ADR-0004](0004-float-search-integer-truth.md).
