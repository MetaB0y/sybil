---
tags: [solver, overview]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Solver landscape

> [!summary] In one paragraph
> Solver implementations share one interface and integer trust boundary. The supported matching core is a fast LP; shared MM capital introduces endogenous price×quantity coupling. [[Retained Cash Solver|`RetainedCashSolver`]] remains the production default. The experimental [[Pacing Bundle Solver|`PacingBundleSolver`]] solves the same convex retained-cash program through a lower-dimensional pacing dual and a fully corrective primal bundle. [[Direct Price-Pacing Dual Solver|`DirectDualConicSolver`]] is an independent price-side certificate/reference whose continuous fill recovery is not yet robust enough for production integer landing.

| Solver | Feature | MM-budget approach | Role |
|---|---|---|---|
| [[Retained Cash Solver|`RetainedCashSolver`]] | `retained-cash` | Generalized Frank--Wolfe on affine-to-log MM utility | Production default |
| [[Pacing Bundle Solver|`PacingBundleSolver`]] | `lp` | Fully corrective primal atoms from the convex pacing dual | Research candidate |
| [[LP Solver|`LpSolver`]] | `lp` | Solve, linearize budgets at discovered prices, re-solve once by default | Low-latency baseline |
| [[Conic Solver|`ConicSolver`]] | `conic` | Clarabel exponential-cone formulation, then projection LP | Interior-point reference |
| [[Direct Price-Pacing Dual Solver|`DirectDualConicSolver`]] | `conic` | Price/pacing hinge dual; fill quantities from hinge-row multipliers | Certificate and marginal-face research reference |
| [[MILP Solver|`MilpSolver`]] | `milp` | SCIP MIQCQP or McCormick mode with timeout | Exact/reference route when optimal |
| [[Decomposed Solver|`DecomposedSolver<S>`]] | `lp` | Component solves with proportional-response MM budget coordination | Scaling experiment |

The removed IterLP damped fixed point and forced-step EG implementation did not
have the claimed convergence semantics. Their public types and CLI variants
have been removed; historical protocol v1 remains reproducible at its frozen
source revision. `ConicSolver` in QuasiFisher mode is an independent
exponential-cone formulation of the same objective. Its backend failures remain
failures rather than being replaced by another solver.

```mermaid
flowchart LR
    P["Problem"] --> FILTER["Supported-shape filter"]
    FILTER --> SEARCH["Chosen float search"]
    SEARCH --> LAND["Supporting projection / integer landing"]
    LAND --> INT["Integer fills + prices + net welfare"]
    INT --> VERIFY["sybil-verifier"]
```

Shared machinery includes the HiGHS LP oracle, price normalization from duals,
lexicographic nearest-face projection, and integer rounding. Retained-cash
projections preserve the certified target within the primary supporting face,
re-solve price-dependent budget rows, and finalize only at a budget-consistent
fixed point; the LP-SLP baseline still has a capped trimming path and is
measured as such. MM sells are paced through the paper's sell-to-complementary-buy
reduction, including its exact linear complete-set correction.
`PipelineResult::diagnostics` reports algorithm termination separately from
integer validity: convergence, a configured iteration cap, backend failure,
and projection failure are not interchangeable. `matching-sim` compares
results; `sybil-verifier` decides validity.

The production sequencer enables only `retained-cash`. The `lp` feature adds
the public LP baseline, pacing-bundle solver, and decomposition experiment;
`conic` includes that research surface because its reference implementation
reuses the same LP projection utilities. This build boundary keeps production
coupled to the certified solver it actually invokes without deleting research
implementations.

## Important boundaries

- The payoff-vector domain model is more expressive than current production clearing. Unsupported multi-market/custom shapes are rejected at every boundary.
- Solver libraries may use `f64`; protocol state never trusts those raw values.
- A MILP timeout incumbent is not a proven global optimum.
- Research solvers do not silently return an LP result after numerical failure.
  Explicit delegation exists only where the mathematical objective reduces to
  LP (for example no active log-utility MMs or Conic Linear mode).
- Benchmark rankings belong in the complete preregistered artifacts under
  `benchmarks/solver/results/`, not timeless architecture claims or a selected
  `just compare` run.

## Where this lives

> `crates/matching-solver/src/solver.rs` — shared interface and supported-shape filtering  
> `crates/matching-solver/src/` — implementations  
> `crates/matching-sim/` — comparison harness
> `benchmarks/solver/` — preregistered empirical protocol and retained results

## See also

- [[The LP Core]]
- [[MM Budget Constraint]]
- [[Direct Price-Pacing Dual Solver]]
- [[Four-Layer Verification]]
