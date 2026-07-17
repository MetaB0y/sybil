---
tags: [solver, fisher-market, market-maker, research]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Pacing bundle solver

> [!summary] In one paragraph
> `PacingBundleSolver` is the fully corrective core of the production retained-cash clearer and solves the same zero-temperature program as [[Retained Cash Solver|`RetainedCashSolver`]]. It uses the variational pacing representation to work in one effective dimension per market maker, retains matching-LP optima as primal atoms, and fully corrects their convex weights. A HiGHS dual bound gives a genuine continuous retained-cash certificate. `ProductionSolver` wraps it in exact economic-connectivity routing.

For budget `B > 0` and MM weighted fill `U`, the shifted retained-cash utility
satisfies

```text
psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha).
```

Fixing all pacing factors `alpha_k` leaves the ordinary [[The LP Core|matching
LP]] with every MM's reduced value shaded by its one common factor. One oracle
solution therefore has two roles: it supplies a cutting plane for the convex
pacing dual and a feasible atom for the primal allocation.

The default oracle is reusable HiGHS.
`LinearOracleBackend::StructuralPriceSweep` is an experimental exact
alternative for supported one-hot single-market books. It obtains prices by
sorting piecewise-linear hinge segments and uses complementary slackness plus
balanced marginal-face recovery to produce the primal atom. Final price
discovery and integer landing still use HiGHS. The backend comparison and
failed face-selection variants are recorded in
`design/solver-experiments/structural-price-sweep-oracle.md`.

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

The default global certificate combines a `1e-8` relative gap with a
`100,000`-nanodollar (`$0.0001`) absolute gap. The previous `$0.001` absolute
gap could dominate the relative criterion on small books and stop after one
atom, leaving the subsequent integer landing far from a good face. The tighter
default was selected with a replay tolerance sweep plus the complete synthetic
development matrix; the sweep and its non-monotone landing trade-off are
recorded in `design/solver-experiments/sequencer-replay-corpus.md`.

## Integer landing

The continuous mixture is not protocol state. A final pacing-supported LP first
discovers the primary matching objective and market duals. A second,
lexicographic LP stays on that primary optimal face and minimizes L1 distance
to the certified mixture. This prevents an arbitrary basis on a degenerate face
from replacing the allocation—as happened in a development case with 67.9%
retained-objective loss. Published prices always come from the primary solve;
the auxiliary distance-row duals are never treated as market prices.

Normally scaled books use the exact primary face and check its activity
directly. Deliberately wide billion-unit books instead use a `1e-8` relative
near-face band: HiGHS can report an exact auxiliary optimum there while the
face row is materially infeasible. Before budget projection, the implementation
compares the nearest-face, primary-basis, and certified-target integer
candidates under the primary prices. It first finds the smallest
minting-duality residual, excludes candidates more than one microdollar above
that value, and maximizes retained-cash objective inside the resulting
support-equivalence band. It fails explicitly if even the best support residual
exceeds $0.05. This gate does not call another solver or replace the primary
price system. Auxiliary utility bands were tested and rejected because their
shadow prices can invalidate the published market duals. The policy comparison
is recorded in
`design/solver-experiments/objective-aware-landing-selection.md`.

The localized landing normally caps each order at the ceiling of its recovered
continuous fill. When that result fails or loses more than one basis point of
retained objective, the solver retries the same final tangent face with the
original order bounds, while leaving zero-budget MM orders closed. The expanded
candidate is retained only if its actual verifier-ready retained objective is
higher. This conditional retry can choose an integer-friendly representative
of a degenerate certified face without adding another core solver, changing
published prices, or weakening the support and hard-budget gates.

After rounding, the projection re-linearizes any violated hard-budget rows and
resolves to a budget-consistent fixed point. Exhaustion is an explicit
`PostProcessingFailure`; no different core solver is substituted. The benchmark
reports retained-objective landing loss, L1 allocation movement, whether budget
quantities were trimmed, and `|C_0(D) - p·D|`, so post-price mutation cannot
masquerade as a better allocation.

Zero-budget MM orders are disabled in the retained-cash oracle. The theorem's
pacing identity assumes `B > 0`; admitting a zero-budget order as a free atom
would create a degenerate supply direction even though its landed hard budget
permits no capital consumption.

## Evidence boundary

The broad pacing protocol remains development evidence because its seeds were
observed while the algorithm and landing were repaired. Production promotion
instead used the separately frozen
`benchmarks/solver/protocol-bundle-promotion-v1.json`: 136 complete rows on
previously untouched seeds, with 68 verifier-valid results per solver. The
exact bundle improved the landed retained-cash objective in 49 paired cases,
tied 19, and regressed in none; iteration caps fell from 22 to 2 and P99
latency from 2,317 ms to 477 ms. The immutable result is
`benchmarks/solver/results/2026-07-17-bundle-promotion-v1/`; the full decision
record is `design/solver-experiments/exact-component-decomposition.md`.

## Where this lives

> `crates/matching-solver/src/pacing_bundle_solver.rs` — bundle master, primal recovery, and certificate  
> `crates/matching-solver/src/lp_solver.rs` — reusable oracle, dual upper bound, and safe projection  
> `benchmarks/solver/protocol-pacing-development.json` — development-only stress design

## See also

- [[Retained Cash Solver]]
- [[MM Budget Constraint]]
- [[Solver Landscape]]
- [[LP Duality and Clearing Prices]]
