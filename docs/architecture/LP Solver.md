---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-03-15
---

The LP Solver is the production default — fastest and highest welfare across all benchmarks. It solves the [[The LP Core|core LP]] via HiGHS (a state-of-the-art open-source LP solver), then handles [[MM Budget Constraint|MM budget constraints]] through iterative SLP (Sequential Linear Programming) budget shading.

The approach works in rounds. First, solve the LP ignoring MM budgets entirely. Then check each MM's capital usage at the resulting clearing prices. If any MM is over budget, shade (proportionally reduce) their order fills and re-solve. Repeat until all budgets are satisfied or convergence is reached. Because there are only 2-10 MMs, this typically converges in 2-3 iterations. The key insight is that shading one MM's orders barely affects clearing prices for other markets, so the iterative approach works well in practice.

Entropy smoothing is added to the LP objective to break ties deterministically. When multiple orders have the same limit price, the LP has degenerate optima — the simplex method could return any of them. A small entropy term `epsilon * sum(q_i * log(q_i))` penalizes extreme allocations, spreading fills more evenly across same-priced orders. The epsilon is tiny enough to never affect welfare-optimal decisions but large enough to ensure reproducible results.

In benchmarks, the LP solver achieves the highest welfare with 100% MM budget utilization. It beats the [[EG Solver]] by ~2% in welfare and is over 10x faster. Against the [[Conic Solver]] (QuasiFisher mode), the welfare gap is under 1% — LP welfare is actually higher because SLP budget shading is more aggressive than the conic relaxation. See `design/solver-benchmarks.md` for current numbers.

## Key Properties
- HiGHS LP solver — simplex or interior-point, open-source, C++ backend
- Iterative SLP for MM budgets — converges in 2-3 rounds
- Entropy smoothing for deterministic tie-breaking
- Fastest solver across all problem sizes
- Highest welfare across all benchmarks
- 100% MM budget utilization

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — LP construction, SLP loop, entropy smoothing

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[The LP Core]] — the LP being solved
- [[MM Budget Constraint]] — how SLP shading handles budgets
- [[EG Solver]] — alternative Fisher market formulation (13x slower, -2.4% welfare)
- [[Conic Solver]] — interior-point alternative (1.7x slower, -0.5% welfare)
