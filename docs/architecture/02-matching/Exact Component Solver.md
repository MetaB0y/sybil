---
tags: [solver, decomposition]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Exact component solver

> [!summary] In one paragraph
> `ExactComponentSolver<S>` finds economically independent liquidity clusters and solves each with the same inner solver. Markets are joined by a market group, a spanning or conditional order, or an MM budget shared by orders on both markets. The retained-cash objective and all matching constraints are additive after this coarsening, so the split changes neither the mathematical problem nor any budget. Connected and strongly unbalanced books delegate directly to avoid setup and landing overhead.

The component graph is built with union-find over markets. It adds one
hyperedge for each:

- categorical market group;
- order's active-market set plus any price-condition market; and
- MM constraint's full set of active order markets.

Every order and MM constraint therefore belongs wholly to one resulting
component. Unlike [[Decomposed Solver]], the exact route never splits an MM
budget, drops a spanning order, or runs a coordination fixed point. Each
component crosses the normal integer landing boundary independently; the
combined result is then checked against the original problem and global MM
budgets.

Multiple solver setup and landing phases can cost more than they save on a
tiny detached tail. The current research router splits only when the largest
component contains at most 80% of all orders. This threshold affects execution
cost only: delegation solves the original monolithic problem and preserves the
same semantics. Under the `parallel` feature, selected components run through
Rayon.

The experiment harness records component count and the largest component's
market, order, and MM shares. These are coverage metrics, not solver quality
metrics: a corpus dominated by connected books cannot validate fragmented-book
scaling.

## Where this lives

> `crates/matching-solver/src/exact_components.rs`

## See also

- [[Solver Landscape]]
- [[Decomposed Solver]]
- [[MM Budget Constraint]]
- [[Binary Markets and Market Groups]]
