---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-11
---

The MILP Solver formulates matching plus [[MM Budget Constraint|MM budget constraints]] as a mixed-integer quadratically constrained program and solves it with SCIP through `russcip`. It is the exact/reference route when SCIP reaches a proven optimum; with a timeout it may return the best feasible solution found so far.

The bilinear `price * quantity` terms in MM budgets are handled natively by SCIP's branch-and-bound algorithm. Where the [[LP Solver]] approximates via iterative shading and the [[EG Solver]] relaxes via log-utility absorption, the MILP solver simply gives SCIP the exact non-convex problem and lets it find the global optimum through systematic enumeration with pruning. This is NP-hard in general, but with only 2-10 MMs the branching tree is manageable. A configurable timeout (default 60 seconds) ensures the solver doesn't run forever on pathological instances — it returns the best solution found so far.

The solver is feature-gated because SCIP has external dependencies. Its primary role is benchmarking and validation against faster heuristics. The current implementation, like the LP family, asserts single-market binary orders.

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
