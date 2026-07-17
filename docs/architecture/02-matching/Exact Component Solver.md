---
tags: [solver, decomposition]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Exact component solver

> [!summary] In one paragraph
> `ExactComponentSolver<S>` finds economically independent liquidity clusters and solves each with the same inner solver. Markets are joined by a market group, a spanning or conditional order, or an MM budget shared by orders on both markets. The retained-cash objective and all matching constraints are additive after this coarsening, so the split changes neither the mathematical problem nor any budget. It is an explicit opt-in accelerator and topology benchmark; [[Pacing Bundle Solver|the monolithic pacing bundle]] is the production security baseline.

The component graph is built with union-find over markets. It adds one
hyperedge for each:

- categorical market group;
- order's active-market set plus any price-condition market; and
- MM constraint's full set of active order markets.

Every order and MM constraint therefore belongs wholly to one resulting
component. Unlike [[Decomposed Solver]], the exact route never splits an MM
budget, drops a spanning order, or runs a coordination fixed point. Each
component crosses the normal integer landing boundary independently. Assembly
then canonicalizes fills by admitted order ID, rechecks the original global MM
budgets, and recomputes integer welfare. Canonical ordering is consensus
relevant: component numbering must not leak hash-map iteration order into
account event digests.

Multiple solver setup and landing phases can cost more than they save on a
tiny detached tail. The production router splits only when the largest
component contains at most 80% of all orders. This threshold affects execution
cost only: delegation solves the original monolithic problem and preserves the
same semantics. Under the `parallel` feature, selected components run through
Rayon.

The experiment harness records component count and the largest component's
market, order, and MM shares. These are coverage metrics, not solver quality
metrics: a corpus dominated by connected books cannot validate fragmented-book
scaling.

The frozen adversarial-connectivity audit then exercised the opposite extreme:
one 384-order global maker or one 64-order bridge made every 10,000- and
50,000-order book connected. Wrapped and monolithic bundle results were
identical on all recorded economic, landing, and MM metrics. Router P95
overhead was only 2.81% at 10,000 orders and 0.48% at 50,000, so the router
itself was not the bottleneck. However, the monolith's 50,000-order P50/max was
`82.28/85.97s`, versus the deployed ten-second block interval.

Because an admitted bridge can disable decomposition at will, the frozen rule
removed this optional layer from `ProductionSolver`. The generic exact solver
remains useful when a caller explicitly accepts topology-dependent
acceleration, and its balanced-component evidence remains valid; neither is a
capacity or denial-of-service guarantee.

## Where this lives

> `crates/matching-solver/src/exact_components.rs`  
> `crates/matching-solver/src/production_solver.rs`  
> `crates/matching-solver/src/component_assembly.rs`

## See also

- [[Solver Landscape]]
- [[Decomposed Solver]]
- [[MM Budget Constraint]]
- [[Binary Markets and Market Groups]]
