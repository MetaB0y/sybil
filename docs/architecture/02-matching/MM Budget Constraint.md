---
tags: [concept, economics]
layer: solver
status: current
last_verified: 2026-07-20
---

An explicit market-maker budget is the source of non-convexity in the original
hard-budget formulation. Without it, supported zero-temperature clearing is a
clean [[The LP Core|linear program]]. With it, the model has bilinear side
constraints—one per market maker—that couple primal fills with dual clearing
prices. The general complexity classification is more nuanced than simply
calling every such book NP-hard; thin books can convexify, while adversarial
retail walls preserve a genuinely non-concave frontier.

A market maker submits orders on both sides of multiple markets with a capital budget limiting total risk exposure. The capital consumed by each fill depends on the clearing price: buying YES at price p costs `p * qty_units / SHARE_SCALE` in capital, while selling YES costs `(1 - p) * qty_units / SHARE_SCALE`. Since the clearing price p is itself a dual variable determined by the optimization, the budget constraint is `sum(price_m * qty_units_i * coefficient_i / SHARE_SCALE) <= budget_k` — a product of a primal quantity and a dual price. This bilinear coupling means the feasible region is no longer convex, and multiple local optima become possible. At the engine API boundary, `MmSide::capital_needed` receives the price of the outcome actually traded: either outcome buy consumes its fill price and either outcome sell consumes one minus its fill price. LP internals that hold a YES price convert NO orders before calling it.

The production path changes the objective instead of imposing this non-convex
constraint directly. [[Retained Cash Solver]] uses the paper's exact
affine-to-log reduced-form utility: at its optimum, one pacing multiplier per MM
makes the shared budget self-enforcing. Generalized Frank–Wolfe then has a
provable convergence certificate while retaining the matching LP as its linear
oracle. [[LP Solver|LP-SLP]] remains a one-pass budget-linearization baseline;
[[Pacing Bundle Solver|the pacing bundle]] is a fully corrective alternative
for the same convex objective; [[Conic Solver|Conic]] is an independent exponential-cone reference;
[[MILP Solver|MILP]] attacks the original bilinear model on small instances;
[[Decomposed Solver|decomposition]] coordinates a spanning MM across components.

## Runtime actor policy

The off-chain shared MM actor submits its configured flash-liquidity budget
unchanged on every non-empty live block. A separate directional-exposure cap
switches quote generation to risk-reducing orders; it never shrinks the budget
to zero. Matched YES+NO inventory is a redeemable complete set, not directional
exposure. The actor submits atomic paired sells whose limits sum to one dollar
minus one nano so settlement burns the pair and returns collateral. Baseline
cash-backed quotes are selected across the eligible catalog before compaction
or extra inventory orders consume the remaining order capacity.

Owner-process readiness requires fresh quote progress, nonzero eligibility,
full selected-market coverage, normal-mode two-sided coverage, and an accepted
submission within two observed blocks. See
[`market-maker-liveness.md`](../../runbooks/market-maker-liveness.md).

## Key Properties
- In YES-price coordinates: BuyYes/SellNo = `p_yes * qty_units / SHARE_SCALE`, SellYes/BuyNo = `(1 - p_yes) * qty_units / SHARE_SCALE`; the engine API instead receives each order's actual outcome fill price
- Bilinear: couples primal `q_i` with dual `p_m` and is generally non-convex
- Only 2-10 MMs in practice — the problem is "almost" an LP
- Each solver handles this differently: certified reduced-form pacing, SLP
  shading, exponential cones, or branch-and-bound
- Without the explicit constraint, the supported zero-temperature core is an LP

## Where This Lives
> `crates/matching-engine/src/mm_constraint.rs` — `MmConstraint`, `MmSide`, capital calculation
> `design/problem-statement.md` — formal bilinear budget constraint (Section 6.3)

## See Also
- [[The LP Core]] — the easy problem without budget constraints
- [[LP Solver]] — iterative SLP shading approach
- [[Retained Cash Solver]] — production reduced-form algorithm and certificate
- [[EG Solver]] — removal status of the historical EG entry point
- [[Solver Landscape]] — how each solver handles budgets differently
