---
adr: 0020
title: Select canonical maximum-entropy clearing prices
status: Accepted
date: 2026-07-18
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0020 — Select canonical maximum-entropy clearing prices

## Context

The zero-temperature retained-cash auction can have a unique allocation but a
face of supporting prices. HiGHS and Clarabel may return different endpoints
of that face, even though the endpoints have the same primary objective.
Prices are nevertheless protocol state: they determine fill cash flow, MINT
cash, MM capital use, the public tape, and escape valuation.

Choosing a dual basis is therefore not an economic rule. Previous marks and
external prices cannot be validity inputs either. A residual-order heuristic
is also incomplete: an MM quote that appears unfilled may be unavailable
because one shared cross-market budget shades all of that MM's quotes.

The retained-cash paper proves the relevant economic refinement. Positive
minting temperature has a unique price, and as temperature vanishes that price
converges to the unique maximum-entropy point of the zero-temperature
price-optimal face.

## Decision

Sybil first minimizes any residual-KKT relaxation required by integer landing,
then selects the exact integer maximum-entropy point of that minimally relaxed
retained-cash price-support face.

For the production-supported one-market, binary, one-hot order domain:

1. Normalize every order to a bound on its market's YES price. Filled
   quantities use the literal order limit because integer landing must remain
   individually rational. Remaining MM quantity uses the exact rational pacing
   factor `alpha = min(1, B / U)`, where `U` is recomputed from landed fills and
   the same factor applies to every order in that MM's cross-market constraint.
2. Filled quantity supplies the ordinary literal limit-valid support interval.
   Remaining executable quantity supplies the complementary paced KKT bound. A
   one-sided residual therefore selects its economically relevant boundary.
   If integer quantity landing makes residual bounds incompatible, minimize
   the common integer relaxation of every residual bound in that connected
   price component. Filled-order limits and minting constraints remain hard.
3. Intersect those bounds with exact minting complementarity. An independent
   binary market is free only when its landed YES and NO net demands tie.
   Within a categorical group, only markets attaining the active net-demand
   maximum may carry positive price; their prices sum to one when the maximum
   is positive and at most one otherwise.
4. Each minimally relaxed component is a box intersected with a simplex. Its
   maximum-Shannon-entropy integer point is the deterministic water-fill:
   equalize all unclamped outcome probabilities, then assign indivisible
   one-nano remainders by ascending market id. The implicit complementary
   outcome participates in slack simplexes. Thus exact KKT faces use the
   paper's maximum-entropy rule unchanged; only integer inconsistency invokes
   the preceding minimax objective.
5. Reprice every fill at that result and require exact order-limit, hard MM
   budget, settlement, and minting checks. Integer capital rounding may reject
   an otherwise continuous allocation; it may not tilt the canonical price.

`matching-engine` owns this pure integer rule. All production-capable solvers
call it after quantity landing, and `sybil-verifier` recomputes it from the
witness. The OpenVM guest inherits the same verifier code. The sequencer owns
no price policy. A market receives a fresh consensus clearing price only when
it has a nonzero fill or is the condition market of a positively filled
conditional order. A filled condition fixes one active branch and contributes
the exact strict integer bound `p > threshold` or `p < threshold`; the
condition market is canonicalized in the same batch, so a historical mark
never decides activation. Unfilled conditional orders do not constrain the
fixed allocation's selected branch. Existing last-clearing prices may be
carried unchanged in block state for valuation and continuity, but the
verifier rejects changing any other market without a fill. Historical prices
are never inputs to fresh selection.

The current production domain does not include general payoff-vector orders.
Endogenous price-condition activation makes the primary allocation problem a
union of faces, so production admission still rejects conditions until a
solver models that allocation choice. The canonical selector and verifier do,
however, support the branch already fixed by a positive landed conditional
fill: its activation inequality is another integer box bound. This preserves
validity for historical/internal witnesses without pretending that the current
primary solver optimizes conditional allocation. General payoff vectors remain
rejected rather than silently projected onto binary marginals.

## Alternatives considered

- **Use the LP/conic dual returned by the primary solver.** Rejected because a
  numerical basis is not deterministic across equivalent solvers and can pick
  an economically extreme endpoint.
- **Residual buy selects high, residual sell selects low, otherwise midpoint.**
  This is the visible behavior of the KKT bounds in a simple call auction, but
  not a complete definition. Raw quantity misclassifies budget-blocked MM
  liquidity and does not define categorical-group prices.
- **Always use the midpoint of filled-order limits.** Rejected because it can
  leave positive-surplus residual orders unfilled, violate minting
  complementarity, or overspend a shared MM budget.
- **Drop contradictory residual bounds as “balanced.”** Rejected because it
  erases information and lets a tiny opposite quote neutralize real pressure.
  Minimal common relaxation retains both bounds and is exactly checkable.
- **Lexicographically minimize prices.** Deterministic but economically biased
  toward one outcome and still selects an endpoint.
- **Run a small positive-temperature auction.** It gives unique prices but
  changes the primary objective by a non-zero subsidy. Maximum entropy on the
  zero-temperature face is exactly its zero-subsidy limit.
- **Use previous marks or an external reference.** Rejected because history and
  off-chain data would become consensus inputs and make genesis behavior
  special.

## Consequences

**Good:** prices no longer depend on floating-point dual choice, order
permutation, or solver backend; one-sided executable residual liquidity gets
the usual call-auction boundary; balanced books receive the least-informative
price consistent with the retained-cash economics; shared MM scarcity is
handled by one cross-market factor rather than local quote counting.

**Costs / constraints:** canonical price verification is a new
validity-critical check and changes the OpenVM guest commitment. Fresh genesis
and guest repinning are required. Some numerically landed allocations that
were formerly accepted may fail exact canonical support or the hard budget
check and must be relanded. General payoff vectors and endogenous conditional
allocation remain unsupported by production clearing. Canonical verification
does support the active branch of an already-landed conditional fill.

**Follow-ups:** generalize the same invariant only when combinatorial clearing
has an exact guest-checkable price-face certificate; expose support bounds and
the residual classification as non-consensus diagnostics.
