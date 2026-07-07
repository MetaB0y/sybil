---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-03-15
---

The EG (Eisenberg-Gale) Solver reformulates the matching problem as a Fisher market. Instead of treating [[MM Budget Constraint|MM budgets]] as explicit constraints, it absorbs them into the objective function via logarithmic utility. This is theoretically elegant — the budget constraint disappears entirely, replaced by a term `B_k * ln(U_k)` in the objective, where `B_k` is the MM's budget and `U_k` is their total utility from fills.

The Fisher market interpretation is: each market maker is a "buyer" with budget `B_k`, and the orders are "goods" being allocated. The Eisenberg-Gale program maximizes the Nash social welfare — `sum(B_k * ln(U_k))` — which is known to produce competitive equilibrium allocations. At equilibrium, each MM's spending on orders exactly equals their budget, and prices clear all markets. The Frank-Wolfe (conditional gradient) method is used to solve this convex program.

The tradeoff is performance. In benchmarks, the EG solver is roughly 13x slower than the [[LP Solver]] with ~2.4% lower welfare. The welfare gap comes from the Fisher objective forcing MMs to deploy capital even on marginal orders: without a cash variable (the "slack" that the [[Conic Solver|QuasiFisher]] mode adds), MMs can't efficiently throttle spending. MM 2 uses only 75% of its budget, reflecting suboptimal allocation rather than efficient capital preservation. The theoretical foundation connecting Fisher markets to the matching problem is laid out in `paper.typ` in `~/github/prediction-markets-are-fisher-markets/` (pointer `design/math-papers.md`).

## Key Properties
- Eisenberg-Gale / Fisher market formulation
- MM budgets absorbed into `B_k * ln(U_k)` objective — no explicit budget constraints
- Solved via Frank-Wolfe (conditional gradient) method
- Produces competitive equilibrium allocations (Nash social welfare)
- ~13x slower than [[LP Solver]] with ~2.4% welfare gap (see `design/solver-benchmarks.md`)
- No cash variable → MMs can't efficiently throttle capital

## Where This Lives
> `crates/matching-solver/src/eg_solver.rs` — Fisher market formulation and Frank-Wolfe solver

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[MM Budget Constraint]] — the constraint this solver absorbs into the objective
- [[Conic Solver]] — adds a cash variable to fix the budget allocation issue
