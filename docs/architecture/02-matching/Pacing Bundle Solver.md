---
tags: [solver, fisher-market, market-maker, research]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-13
---

# Pacing bundle solver

> [!summary] In one paragraph
> `PacingBundleSolver` is a research implementation of the same zero-temperature retained-cash program as [[Retained Cash Solver|`RetainedCashSolver`]]. It uses the variational pacing representation to work in one effective dimension per market maker, retains matching-LP optima as primal atoms, and fully corrects their convex weights. A HiGHS dual bound gives a genuine continuous retained-cash certificate. Development evidence is promising, but this is not yet the production default or held-out paper evidence.

For budget `B > 0` and MM weighted fill `U`, the shifted retained-cash utility
satisfies

```text
psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha).
```

Fixing all pacing factors `alpha_k` leaves the ordinary [[The LP Core|matching
LP]] with every MM's reduced value shaded by its one common factor. One oracle
solution therefore has two roles: it supplies a cutting plane for the convex
pacing dual and a feasible atom for the primal allocation.

The implementation is a simplicial-decomposition / fully corrective bundle
method. It keeps the distinct LP atoms, represents the current allocation as a
convex mixture, and performs exact one-dimensional pairwise line searches in a
small restricted master. It then asks the reusable LP oracle for the best new
atom. The active set is normally much smaller than the order count; the
development protocol records it instead of treating that claim as timeless.

## Certificate

The returned primal is the retained atom mixture, so it is feasible for the
continuous matching polytope. At its current pacing factors the final LP call
supplies a conservative Lagrangian upper bound derived from HiGHS row duals,
reduced costs, and finite analytical column bounds. The difference between the
best global upper bound and the mixture's retained-cash objective is the
reported certificate.

The oracle's returned primal objective is not used as an exact upper bound.
Floating-point LP tolerances can make that shortcut optimistic. This distinction
is covered by regression tests and is shared with RC-FW.

## Integer landing

The continuous mixture is not protocol state. Landing caps fills at the ceiling
of the mixture and solves the final pacing-supported LP. If rounded quantities
violate a hard budget at the discovered prices, the projection
re-linearizes those budget rows and resolves. It finalizes only after reaching
a budget-consistent fixed point; otherwise it returns an explicit
`PostProcessingFailure`.

No different core solver is substituted. A supporting LP can select another
point on a degenerate optimal face, so the landed objective need not equal the
certified continuous mixture's objective. Auxiliary utility-band rows were
tested and rejected: their shadow prices can invalidate the published market
duals. The benchmark therefore reports integer landing loss and
`|C_0(D) - p·D|`, so post-price fill mutation cannot masquerade as a better
allocation.

Zero-budget MM orders are disabled in the retained-cash oracle. The theorem's
pacing identity assumes `B > 0`; admitting a zero-budget order as a free atom
would create a degenerate supply direction even though its landed hard budget
permits no capital consumption.

## Evidence boundary

The checked-in pacing protocol uses development seeds and was observed while
the algorithm and landing were being repaired. It tests order-count and
market-maker-count scaling separately and retains every cap and failure. Its
results can guide engineering and the design of a future preregistration; they
cannot be called held out. [[Retained Cash Solver|RC-FW]] remains the production
default until a frozen comparison on untouched instances justifies a change.

## Where this lives

> `crates/matching-solver/src/pacing_bundle_solver.rs` — bundle master, primal recovery, and certificate  
> `crates/matching-solver/src/lp_solver.rs` — reusable oracle, dual upper bound, and safe projection  
> `benchmarks/solver/protocol-pacing-development.json` — development-only stress design

## See also

- [[Retained Cash Solver]]
- [[MM Budget Constraint]]
- [[Solver Landscape]]
- [[LP Duality and Clearing Prices]]
