---
tags: [solver, fisher-market, market-maker, research]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Direct price-pacing dual solver

> [!summary] In one paragraph
> `DirectDualConicSolver` is a feature-gated research reference for the same
> retained-cash program as [[Retained Cash Solver|`RetainedCashSolver`]]. It
> optimizes normalized YES prices and MM pacing factors directly, represents
> order demand with hinge epigraphs, and uses exponential cones for
> `log(alpha)`. Hinge-row dual multipliers recover a continuous fill vector.
> The formulation gives a strong independent certificate, but development
> evidence shows that its degenerate continuous fill can be a poor target for
> integer landing, so it is not a production candidate.

For fixed MM pacing factors `alpha`, eliminating each bounded order quantity
from [[The LP Core]] gives the price dual

```text
minimize    sum_i Q_i [c_i(alpha) - payoff_i · p]_+
subject to  p_yes,m + p_no,m = 1
            sum_{m in categorical group g} p_yes,m <= 1
            p >= 0.
```

The retained-cash variational identity

```text
psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha)
```

then produces one joint convex objective in prices and pacing factors. The
Clarabel formulation introduces one nonnegative hinge epigraph per order and
an exponential-cone triple `(log(alpha), 1, alpha)` per active MM. It
reevaluates the exact nonsmoothed hinge/log objective after projecting extracted
prices into the binary and categorical price domain; that value is the reported
continuous upper bound.

The dual multiplier of an order's hinge row lies between zero and its quantity
cap and is a feasible continuous primal fill. This is mathematically sufficient
for a lower bound, but it does not select a unique point on a degenerate optimal
face. The shared supporting-price/nearest-face landing still owns all integer
protocol output and can fail explicitly when no candidate satisfies its
minting-price residual gate.

## Evidence boundary

The 17 July development matrix covered market-like flow, tight shared-capital
flash ladders, numerical range, and MM dimension. The direct cone produced very
tight continuous certificates, but it succeeded on only 53/59 cases and its
worst integer landing lost `0.480789%` despite a `$0.016182` continuous gap.
This isolates marginal-face selection and integer landing as the next research
problem. Complete settings, failures, counterexamples, and comparison metrics
are kept in `design/solver-experiments/price-pacing-dual.md`.

Do not infer held-out performance from that development matrix. Do not replace
an explicit Clarabel or post-processing failure with another solver. The
production sequencer continues to enable only
[[Retained Cash Solver|`RetainedCashSolver`]].

## Where this lives

> `crates/matching-solver/src/direct_dual_conic_solver.rs` — exact cone model, fill recovery, diagnostics  
> `crates/matching-solver/src/price_pacing_dual.rs` — price-domain projection/evaluation and test-only fixed-pacing cross-check  
> `benchmarks/solver/protocol-price-pacing-development.json` — development-only comparison

## See also

- [[Solver Landscape]]
- [[Retained Cash Solver]]
- [[Pacing Bundle Solver]]
- [[MM Budget Constraint]]
- [[LP Duality and Clearing Prices]]
