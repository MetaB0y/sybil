---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-03-15
---

The MILP Solver is the only solver that finds the exact global optimum. It formulates the full matching problem — including [[MM Budget Constraint|MM budget constraints]] — as a Mixed-Integer Quadratically Constrained Program (MIQCQP) and solves it using SCIP, a state-of-the-art academic optimization solver accessed through the `russcip` Rust bindings.

The bilinear `price * quantity` terms in MM budgets are handled natively by SCIP's branch-and-bound algorithm. Where the [[LP Solver]] approximates via iterative shading and the [[EG Solver]] relaxes via log-utility absorption, the MILP solver simply gives SCIP the exact non-convex problem and lets it find the global optimum through systematic enumeration with pruning. This is NP-hard in general, but with only 2-10 MMs the branching tree is manageable. A configurable timeout (default 60 seconds) ensures the solver doesn't run forever on pathological instances — it returns the best solution found so far.

The MILP solver's particular strength is exploiting [[Minting|group minting]] structure. Because it models the full combinatorial space, it can find group minting opportunities that heuristic solvers miss — cases where minting across an entire [[Binary Markets and Market Groups|market group]] enables fills that per-market minting cannot. This advantage grows with the number of markets per group. The solver is feature-gated behind the `milp` feature flag because SCIP has external library dependencies that not all environments can satisfy. It's primarily used for benchmarking and validation: solving a batch with the MILP solver and comparing against the [[LP Solver]] output confirms how close the heuristic is to optimal.

## Key Properties
- SCIP via `russcip` — MIQCQP branch-and-bound
- Exact global optimum (given sufficient time)
- Configurable timeout: `--milp-timeout 60`
- Feature-gated: `--features milp`
- Exploits [[Minting|group minting]] structure better than heuristic solvers
- Primary use: benchmarking and validating heuristic solver quality

## Where This Lives
> `crates/matching-solver/src/milp.rs` — SCIP formulation and solve

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[LP Solver]] — the heuristic solver MILP validates against
- [[MM Budget Constraint]] — the non-convex constraint handled exactly
- [[Minting]] — group minting advantages in the MILP formulation
