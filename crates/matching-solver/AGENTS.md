# `matching-solver`

Optimization implementations for the supported welfare-clearing problem. The
production-supported core is a convex/linear clearing problem; do not describe
the whole crate as NP-hard merely because the optional MILP reference route can
model harder variants.

## Read first

- [[Solver Landscape]] and [[The LP Core]]
- [[Welfare Maximization]] and [[LP Duality and Clearing Prices]]
- [[MM Budget Constraint]]
- The focused note for the solver being changed

## Implementations

| Type | Backend / role |
|---|---|
| `ProductionSolver` | Production facade: exact-connectivity routing around the pacing bundle |
| `RetainedCashSolver` | Independent certified generalized Frank–Wolfe reference with a HiGHS oracle |
| `PacingBundleSolver` | Fully corrective retained-cash core with a HiGHS oracle |
| `LpSolver` | Low-latency risk-neutral baseline plus budget-linearized re-solve |
| `ConicSolver` | Independent Clarabel Linear/Fisher/QuasiFisher reference |
| `DirectDualConicSolver` | Price-side Clarabel retained-cash certificate and marginal-face research reference |
| `MilpSolver` | Feature-gated SCIP exact/reference route with timeout |
| `DecomposedSolver<S>` | Per-group mirror-descent coordination experiment |
| `ExactComponentSolver<S>` | Exact economic-connectivity decomposition with production balanced-book routing |

## Invariants

- Solvers may search in floating point; landed fills/prices and trusted welfare
  are integers.
- All implementations return `PipelineResult`, but `sybil-verifier` owns
  correctness and the net-of-minting welfare definition.
- Distinguish a MILP incumbent/timeout from a proven optimum.
- Distinguish an RC-FW iteration cap from convergence; only its reported
  generalized Frank–Wolfe gap is a continuous-objective certificate.
- Research-solver availability is separate from candidate conformance: a
  numerical failure must be explicit, while every returned best iterate must
  pass the same integer/verifier checks as production.
- Unsupported multi-market shapes must not become production-executable merely
  because a solver can represent them.
- Preserve uniform-price, quantity/limit, group, MM-budget, rounding, and
  integer-landing behavior across solvers.

Main files are `solver.rs`, `result.rs`, `production_solver.rs`, each
`*_solver.rs`, `milp.rs`, `decomposed.rs`, and `exact_components.rs`; `viz.rs`
is reporting support.

```bash
cargo test -p matching-solver --features retained-cash
cargo test -p matching-solver
cargo test -p matching-solver --features lp,conic,milp
```
