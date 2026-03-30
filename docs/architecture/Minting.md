---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-03-15
---

Minting is how the exchange creates new shares when there aren't enough sellers to match buyers. Because a binary market's YES and NO shares always sum to $1 at resolution, the protocol can safely create a matched pair (1 YES + 1 NO) for $1. This is per-market minting — it shows up as a decision variable `mint_m` in the [[The LP Core|LP]] and appears as a cost term in the [[Welfare Maximization|welfare objective]].

Group minting is the powerful generalization. For a [[Binary Markets and Market Groups|market group]] of K mutually exclusive markets, creating 1 YES share on every market in the group costs just $1 total (since exactly one will pay out). This is K times cheaper per YES share than per-market minting. If an election has 5 candidates, group minting creates 5 YES shares for $1 instead of $5. The solver uses group minting variables `gmint_g` to exploit this structural advantage, and the [[MILP Solver]] is particularly good at finding group minting opportunities that heuristic solvers miss.

In the LP formulation, minting costs are negative terms in the welfare objective: `-$1 * sum(mint_m) - $1 * sum(gmint_g)`. This prevents the solver from minting unboundedly — it will only mint when the welfare gain from enabling additional fills exceeds the minting cost. Through [[LP Duality and Clearing Prices|LP duality]], the minting stationarity conditions give you price normalization for free: `YES_price + NO_price <= $1` per market, with equality when minting is active. For groups, it gives `sum(YES_prices) <= $1` with equality when group minting is active.

## Key Properties
- Per-market: 1 YES + 1 NO = $1, variable `mint_m >= 0`
- Group: 1 YES on each of K markets = $1, variable `gmint_g >= 0`
- Group minting is K times cheaper per YES share
- Minting cost in objective prevents unbounded creation
- [[LP Duality and Clearing Prices|Dual stationarity]] enforces price normalization automatically

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — minting variables and constraints in the LP
> `design/problem-statement.md` — formal definition of minting mechanics

## See Also
- [[Binary Markets and Market Groups]] — groups enable cheaper group minting
- [[The LP Core]] — how minting variables enter the LP formulation
- [[MILP Solver]] — exploits group minting structure better than heuristic solvers
