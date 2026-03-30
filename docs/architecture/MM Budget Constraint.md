---
tags: [concept, economics]
layer: solver
status: current
last_verified: 2026-03-15
---

The market maker budget constraint is the sole source of NP-hardness in the matching problem. Without it, everything is a clean [[The LP Core|linear program]]. With it, the problem becomes an LP with bilinear side constraints — one per market maker — that couple primal variables (fill quantities) with dual variables (clearing prices).

A market maker submits orders on both sides of multiple markets with a capital budget limiting total risk exposure. The capital consumed by each fill depends on the clearing price: buying YES at price p costs `p * qty` in capital, while selling YES costs `(1 - p) * qty`. Since the clearing price p is itself a dual variable determined by the optimization, the budget constraint is `sum(price_m * qty_i * coefficient_i) <= budget_k` — a product of a primal quantity and a dual price. This bilinear coupling means the feasible region is no longer convex, and multiple local optima become possible.

The saving grace is that there are very few market makers — typically 2 to 10 in any realistic batch. This small number makes the problem amenable to specialized methods. The [[LP Solver]] uses iterative SLP (Sequential Linear Programming) budget shading: solve the LP, compute MM capital usage at the resulting prices, shade (reduce) fills that exceed budget, re-solve, and repeat until convergence. The [[EG Solver]] absorbs budgets into a Fisher market log-utility objective, avoiding explicit budget constraints entirely. The [[MILP Solver]] handles them exactly via SCIP's branch-and-bound. The [[Conic Solver]] uses an exponential cone formulation with a cash variable that acts as a numerical buffer.

## Key Properties
- Capital usage: BuyYes/SellNo = `price * qty`, SellYes/BuyNo = `(1 - price) * qty`
- Bilinear: couples primal `q_i` with dual `p_m` — non-convex
- Only 2-10 MMs in practice — the problem is "almost" an LP
- Each solver handles this differently: SLP shading, log-utility absorption, branch-and-bound, conic relaxation
- Without this constraint, the problem is polynomial-time

## Where This Lives
> `crates/matching-engine/src/mm_constraint.rs` — `MmConstraint`, `MmSide`, capital calculation
> `design/problem-statement.md` — formal bilinear budget constraint (Section 6.3)

## See Also
- [[The LP Core]] — the easy problem without budget constraints
- [[LP Solver]] — iterative SLP shading approach
- [[EG Solver]] — Fisher market absorption of budgets
- [[Solver Landscape]] — how each solver handles budgets differently
