---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-18
---

The MILP Solver formulates matching plus [[MM Budget Constraint|MM budget constraints]] as a mixed-integer quadratically constrained program and solves it with SCIP through `russcip`. It is the exact/reference route when SCIP reaches a proven optimum; with a timeout it may return the best feasible solution found so far.

The bilinear `price * quantity` terms in MM budgets are handled natively by
SCIP's branch-and-bound algorithm. Where the [[LP Solver]] uses SLP shading and
[[Retained Cash Solver]] solves a convex reduced-form objective, the MILP gives
SCIP the original non-convex model. This is NP-hard in general; small references
can prove global optimality, while realistic or adversarial books may exhaust a
configured timeout. A timeout incumbent is never described as exact.

The solver is feature-gated because SCIP has external dependencies. Its
primary role is benchmarking and validation against faster convex/heuristic
paths. The formulation scales prices to dollars and monetary objectives before
calling SCIP; raw nanos made nonlinear feasibility numerically unreliable.
SCIP's price variables constrain its allocation but are not published. Landed
fills pass through the same [[LP Duality and Clearing Prices|canonical integer
price selector]] as the convex solvers, followed by a bounded hard-budget
fixed point.

## Key Properties
- SCIP via `russcip` — MIQCQP branch-and-bound
- Exact global optimum only when SCIP reports `Optimal`
- Configurable timeout: `--milp-timeout 60`; timeout remains explicit
- Feature-gated: `--features milp`
- Exploits [[Minting|group minting]] structure better than heuristic solvers
- Primary use: benchmarking and validating heuristic solver quality

## Where This Lives
> `crates/matching-solver/src/milp.rs` — SCIP formulation and solve

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[Retained Cash Solver]] — the convex approximation route
- [[LP Solver]] — the risk-neutral SLP baseline
- [[MM Budget Constraint]] — the non-convex constraint handled exactly
- [[Minting]] — group minting advantages in the MILP formulation
