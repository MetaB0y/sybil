---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-11
---

The EG (Eisenberg-Gale) Solver reformulates the matching problem as a Fisher market. Instead of treating [[MM Budget Constraint|MM budgets]] as explicit constraints, it absorbs them into the objective function via logarithmic utility. This is theoretically elegant — the budget constraint disappears entirely, replaced by a term `B_k * ln(U_k)` in the objective, where `B_k` is the MM's budget and `U_k` is their total utility from fills.

The Fisher market interpretation is: each market maker is a "buyer" with budget `B_k`, and the orders are "goods" being allocated. The Eisenberg-Gale program maximizes the Nash social welfare — `sum(B_k * ln(U_k))` — which is known to produce competitive equilibrium allocations. At equilibrium, each MM's spending on orders exactly equals their budget, and prices clear all markets. The Frank-Wolfe (conditional gradient) method is used to solve this convex program.

The tradeoff is a different objective and iterative Frank–Wolfe search. Unlike QuasiFisher, the formulation has no explicit cash/slack variable. It is a reference implementation rather than the production default; current performance belongs in `just compare` output. The theory pointer is `design/math-papers.md`.

## Key Properties
- Eisenberg-Gale / Fisher market formulation
- MM budgets absorbed into `B_k * ln(U_k)` objective — no explicit budget constraints
- Solved via Frank-Wolfe (conditional gradient) method
- Produces competitive equilibrium allocations (Nash social welfare)
- No cash variable → MMs can't efficiently throttle capital

## Where This Lives
> `crates/matching-solver/src/eg_solver.rs` — Fisher market formulation and Frank-Wolfe solver

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[MM Budget Constraint]] — the constraint this solver absorbs into the objective
- [[Conic Solver]] — adds a cash variable to fix the budget allocation issue
