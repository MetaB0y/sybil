---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-11
---

# Decomposed solver

> [!summary] In one paragraph
> `DecomposedSolver<S>` partitions markets into independent components, delegates each component to an inner `Solver`, and coordinates any MM budget spanning components by proportional response on deployed value. It is an experimental scaling route; cross-component orders are unsupported and filtered.

For each spanning MM, the coordinator allocates budget across components, solves them, measures deployed value `V_k^m = U_k^m + s_k^m`, and updates shares proportionally. At a fixed point the MM has equal scarcity `B_k^m / V_k^m` across its active components, matching the component restrictions of the monolithic optimum when no order crosses components.

Rayon may solve independent components concurrently under the `parallel` feature. A single component delegates directly to the inner solver.

## Boundaries

- Wraps any `Solver`.
- Uses proportional response, not the older objective-value/mirror-descent surrogate.
- Drops/rejects cross-component orders rather than mis-modeling them.
- Remains an experiment, not the sequencer default.

## Where this lives

> `crates/matching-solver/src/decomposed.rs`

## See also

- [[Solver Landscape]]
- [[Binary Markets and Market Groups]]
- [[MM Budget Constraint]]
