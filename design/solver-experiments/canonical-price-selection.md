# Canonical clearing-price selection experiments

## Scope and acceptance policy

This ledger records the issue
[#190](https://github.com/MetaB0y/sybil/issues/190) investigation into
non-unique retained-cash equilibrium prices. The allocation objective is
primary. A price-selection candidate is acceptable only if it:

1. selects on the same landed allocation's equilibrium-price face;
2. is exactly reproducible with integer arithmetic by the verifier and guest;
3. respects filled limits, minting/group complementarity, and shared MM
   pacing;
4. is independent of solver backend, floating dual basis, order permutation,
   and thread count.

The motivating binary cross had a seller at 20 and buyer at 70. The old HiGHS
dual basis published 20 even though every price in `[20, 70]` supported the
same fills. A second observed support interval was approximately
`[22.5, 54.9]`.

## CPS-001 — raw residual-quantity tie-break

- Date: 2026-07-18
- Status: rejected as the protocol definition
- Hypothesis: residual buy quantity should select the upper feasible boundary,
  residual sell quantity the lower boundary, and a balance the midpoint.
- Counterexample: an unfilled MM quote is not necessarily executable. One
  shared cross-market budget induces one pacing factor across all of that MM's
  orders, so raw local quantity can report pressure that the retained-cash
  optimum has already priced out.
- Decision: retain the intuitive one-sided behavior as a consequence of KKT
  support bounds, but do not make raw imbalance the economic objective.

## CPS-002 — maximum entropy on the retained-cash price face

- Date: 2026-07-18
- Status: accepted
- Hypothesis: the maximum-Shannon-entropy point of the exact landed
  equilibrium-price face is the canonical zero-temperature price.
- Mathematical basis: the canonical Fisher-market paper proves that the unique
  positive-minting-temperature price converges to the unique maximum-entropy
  point of the zero-temperature price-optimal set.
- Result: in the current one-market binary one-hot domain, KKT and minting
  conditions reduce the face to integer boxes intersected with simplexes.
  Exact deterministic water-filling selects the entropy maximizer without
  logarithms or floating point. The 20–70 and 22.5–54.9 crosses select 50;
  one-sided executable residual liquidity selects its KKT boundary.
- Decision: accepted as ADR-0020 and implemented in `matching-engine`, with
  independent recomputation in `sybil-verifier`.

## CPS-003 — apply paced MM thresholds to landed fills

- Date: 2026-07-18
- Status: rejected
- Hypothesis: the landed integer face should apply the recomputed pacing factor
  to both filled and residual MM quantities.
- Result: continuous retained-cash solutions frequently land a few integer
  units away from exact paced indifference. Applying the recomputed rational
  factor to already-filled quantities made otherwise individually rational
  production landings have an empty face.
- Decision: filled quantities are checked at their literal submitted limits.
  Pacing shades only residual executability. This keeps integer landing behind
  the protocol's literal limit-price boundary while still preventing
  budget-blocked MM quotes from masquerading as pressure.

## CPS-004 — canonicalize once, then trim hard budgets

- Date: 2026-07-18
- Status: rejected
- Hypothesis: canonical repricing followed by the existing deterministic MM
  quantity trim is sufficient.
- Result: trimming only the MM leg changed a market's landed net-demand
  difference after price selection. The old interior price then violated exact
  minting complementarity, and repricing alone could move to an extreme that
  changed capital again.
- Decision: landing is a bounded fixed point. Reprice canonically, trim hard
  budget overflow, trim matched opposite flow to preserve each market's landed
  demand difference, remove unsupported zero-price minting, and repeat. Fail
  explicitly if the quantities and prices do not stabilize.

## CPS-005 — incompatible residual bounds

- Date: 2026-07-18
- Status: fail-closed variant rejected; minimax relaxation accepted
- Hypothesis: any crossed residual KKT bounds should reject the landed
  allocation.
- Result: exact fail-closed behavior rejected four existing retained-cash
  landing regressions. The closest certified-target candidate in the
  multi-MM flash case missed a common residual price by only 95 nanos
  (`499870176 > 499870081`), caused by recomputing a rational pacing factor
  after integer quantity rounding.
- Decision: keep filled limits and minting constraints hard. Lexicographically
  minimize one common integer relaxation of all residual bounds in the price
  component, then maximize entropy on that minimally relaxed face. This
  preserves both sides' information; it does not drop them merely because both
  are present. The exact retained-cash face remains the usual zero-relaxation
  case.

## CPS-006 — canonicalize component prices independently

- Date: 2026-07-18
- Status: rejected
- Hypothesis: exact/decomposed solvers may canonicalize each solved component
  and union their price maps.
- Result: component-local maps omitted markets containing only unfilled orders
  and could not express final cross-component shared-budget repair.
- Decision: merge landed fills first, then run one canonical stabilization over
  the original problem. Component solvers never own protocol price policy.

## CPS-007 — price every market containing an order

- Date: 2026-07-18
- Status: rejected at the consensus boundary
- Hypothesis: because residual liquidity determines a canonical indicative
  price even with no fills, every order-bearing market should receive a fresh
  clearing entry.
- Result: this conflated indicative marking with settlement and invalidated
  empty/no-cross blocks. The sequencer intentionally advances
  `last_clearing_prices` only for markets with nonzero fills.
- Decision: solvers may compute all book prices for diagnostics, but the
  witness requires fresh canonical equality only on filled markets.
  Non-clearing entries must exactly carry the previous committed price, so an
  attacker cannot mutate history by adding an arbitrary entry.

## Verification commands

```bash
cargo test -p matching-engine canonical_price
cargo test -p matching-solver --features lp
cargo test -p sybil-verifier
cargo check -p matching-solver --all-features
```

General payoff-vector orders deliberately fail closed. Production allocation
also continues to reject price-conditioned orders because activation is a
union of faces. After the allocation is fixed, however, a positive conditional
fill fixes one active branch: the canonical verifier treats its strict integer
activation inequality as a hard box bound and canonicalizes the condition
market in the same batch. Unfilled conditional orders do not affect that fixed
branch.
