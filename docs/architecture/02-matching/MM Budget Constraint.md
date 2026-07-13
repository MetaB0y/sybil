---
tags: [concept, economics]
layer: solver
status: current
last_verified: 2026-07-13
---

The market maker budget constraint is the sole source of NP-hardness in the matching problem. Without it, everything is a clean [[The LP Core|linear program]]. With it, the problem becomes an LP with bilinear side constraints — one per market maker — that couple primal variables (fill quantities) with dual variables (clearing prices).

A market maker submits orders on both sides of multiple markets with a capital budget limiting total risk exposure. The capital consumed by each fill depends on the clearing price: buying YES at price p costs `p * qty_units / SHARE_SCALE` in capital, while selling YES costs `(1 - p) * qty_units / SHARE_SCALE`. Since the clearing price p is itself a dual variable determined by the optimization, the budget constraint is `sum(price_m * qty_units_i * coefficient_i / SHARE_SCALE) <= budget_k` — a product of a primal quantity and a dual price. This bilinear coupling means the feasible region is no longer convex, and multiple local optima become possible.

The production path changes the objective instead of imposing this non-convex
constraint directly. [[Retained Cash Solver]] uses the paper's exact
affine-to-log reduced-form utility: at its optimum, one pacing multiplier per MM
makes the shared budget self-enforcing. Generalized Frank–Wolfe then has a
provable convergence certificate while retaining the matching LP as its linear
oracle. [[LP Solver|LP-SLP]] remains a one-pass budget-linearization baseline;
[[Conic Solver|Conic]] is an independent exponential-cone reference;
[[MILP Solver|MILP]] attacks the original bilinear model on small instances;
[[Decomposed Solver|decomposition]] coordinates a spanning MM across components.

## Key Properties
- Capital usage: BuyYes/SellNo = `price * qty_units / SHARE_SCALE`, SellYes/BuyNo = `(1 - price) * qty_units / SHARE_SCALE`
- Bilinear: couples primal `q_i` with dual `p_m` — non-convex
- Only 2-10 MMs in practice — the problem is "almost" an LP
- Each solver handles this differently: certified reduced-form pacing, SLP
  shading, exponential cones, or branch-and-bound
- Without this constraint, the problem is polynomial-time

## Where This Lives
> `crates/matching-engine/src/mm_constraint.rs` — `MmConstraint`, `MmSide`, capital calculation
> `design/problem-statement.md` — formal bilinear budget constraint (Section 6.3)

## See Also
- [[The LP Core]] — the easy problem without budget constraints
- [[LP Solver]] — iterative SLP shading approach
- [[Retained Cash Solver]] — production reduced-form algorithm and certificate
- [[EG Solver]] — compatibility status of the historical EG entry point
- [[Solver Landscape]] — how each solver handles budgets differently
