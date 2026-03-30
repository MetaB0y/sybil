---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-03-15
---

The Decomposed Solver partitions the matching problem by [[Binary Markets and Market Groups|market group]], solves each group independently, and then coordinates [[MM Budget Constraint|MM budget]] allocation across groups using mirror descent. The idea is that if no orders span multiple groups, each group's problem is independent except for the shared MM budgets — so you can solve many small problems instead of one big one.

For single-market order books (no cross-group bundles or spreads), the decomposition is exact: the [[Minting]] cost separates perfectly across groups, and the only coupling is how to split each MM's budget. Mirror descent on budget shares handles this coordination. Mirror descent is a multiplicative weight update algorithm: start with equal budget splits across groups, solve each group, observe which groups were starved for MM capital (indicated by high shadow prices on the budget constraint), and shift budget toward them using exponentiated gradient updates. This converges toward the budget allocation that maximizes total welfare across all groups. The inner solver for each group can be any of the other solvers (LP, Conic, EG) — it wraps a `ComponentSolver` trait. With the `parallel` feature flag, groups are solved concurrently via Rayon.

In practice, the decomposed solver currently underperforms the monolithic solvers. On the reference benchmark (50 groups, ~11K orders), the decomposed conic solver shows a ~7% welfare gap compared to monolithic conic, and is slower due to coordination overhead. The gap reflects imperfect budget coordination: mirror descent hasn't fully converged after the default number of iterations across 50 groups. The solver is also slower on this instance because coordination overhead dominates the savings from smaller per-component problems. Cross-group orders (bundles, spreads) would add structural welfare loss beyond the coordination gap. Known issue: the decomposed LP and EG modes have bugs in MM handling that produce verification violations.

## Key Properties
- Partitions by market group — solves each independently
- Mirror descent coordinates MM budget allocation across groups
- Wraps any `ComponentSolver` (LP, Conic, EG)
- Optional `parallel` feature for concurrent group solving via Rayon
- ~7% welfare gap on reference benchmark — mirror descent hasn't converged (see `design/solver-benchmarks.md`)
- Cross-group orders break decomposition exactness
- Known bugs in decomposed LP/EG MM handling

## Where This Lives
> `crates/matching-solver/src/decomposed.rs` — decomposition, mirror descent, component wrapping

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[LP Solver]] — monolithic solver used as inner component; known bugs in decomposed LP mode
- [[Binary Markets and Market Groups]] — the groups being decomposed over
- [[MM Budget Constraint]] — budget coordination is the key challenge
