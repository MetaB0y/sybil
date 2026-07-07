---
tags: [solver]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-07
---

Sybil has six solver implementations, all sharing the same interface: take a `Problem`, return a `PipelineResult` containing fills, clearing prices, total welfare, and timing data. They differ in how they handle the [[MM Budget Constraint]] — the sole source of difficulty beyond the [[The LP Core|base LP]]. Each is feature-gated so you only compile what you need.

The [[LP Solver]] is the production default. It uses HiGHS to solve the core LP, adds entropy smoothing to break ties deterministically, and handles MM budgets via iterative SLP (Sequential Linear Programming) shading. It's the fastest solver and consistently achieves the highest welfare. The [[EG Solver]] reformulates the problem as a Fisher market using the Eisenberg-Gale objective, absorbing MM budgets into a log-utility function so they never appear as explicit constraints. Elegant but 13x slower than the LP solver. The [[Conic Solver]] uses Clarabel's interior-point method with three configurable objectives (Linear, Fisher, QuasiFisher); QuasiFisher adds a cash variable that prevents numerical blowup. The [[MILP Solver]] uses SCIP to solve the exact problem as a mixed-integer quadratically constrained program — guaranteed optimal but with a timeout for large instances. The [[Decomposed Solver]] partitions the problem by market group and solves each independently, coordinating MM budgets via mirror descent. The **iterative LP solver** (`IterLpSolver`) reaches the EG optimum by fixed-point iteration: at the EG optimum each MM `k` has KKT multiplier `μ_k = min(1, B_k/U_k)`, so it repeatedly solves an LP whose objective boosts positive-welfare MM orders by `(1 + μ_k)`, recomputes `μ`, and re-solves until convergence — conic-level welfare at LP-level robustness.

All solver outputs pass through the same [[Four-Layer Verification|verification pipeline]]. In comparative benchmarks, LP consistently achieves the highest welfare and fastest solve time. Conic (QuasiFisher) is close in welfare (~0.5% gap) at moderate speed cost. EG is significantly slower (~13x) with a small welfare gap (~2.4%). See `design/solver-benchmarks.md` for current numbers.

| Solver | Feature | Method | Speed | Welfare |
|--------|---------|--------|-------|---------|
| **LP** | `lp` | HiGHS + entropy + SLP | Fastest | Highest |
| **EG** | `lp` | Fisher/Eisenberg-Gale | 13x slower | -2.4% |
| **Conic** | `conic` | Clarabel interior-point | 1.7x slower | -0.5% |
| **MILP** | `milp` | SCIP MIQCQP | Timeout-bounded | Exact optimal |
| **Decomposed** | `lp` | Per-group + mirror descent | Varies | -7% |
| **IterLp** | `lp` | Fixed-point LP + EG μ-boost | Multi-solve | ~conic |

```mermaid
flowchart LR
    IN["Problem"]
    subgraph solvers["Solver (pick one)"]
        direction TB
        LP["LpSolver<br/>HiGHS · LP + entropy<br/>production default"]
        EG["EgSolver<br/>Fisher market"]
        CONIC["ConicSolver<br/>Clarabel · conic"]
        MILP["MilpSolver<br/>SCIP · exact"]
        DECOMP["DecomposedSolver<br/>parallel per-group"]
        ITERLP["IterLpSolver<br/>fixed-point LP · EG μ-boost"]
    end
    OUT["PipelineResult<br/>fills · prices · welfare"]
    IN --> solvers --> OUT
```

## Where This Lives
> `crates/matching-solver/src/` — all solver implementations
> `design/solver-benchmarks.md` — comparative benchmarks

## See Also
- [[LP Solver]] — production default, start here
- [[MM Budget Constraint]] — the problem each solver handles differently
- [[The LP Core]] — the easy base problem all solvers share
