---
tags: [concept, solver]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-01
---

Without market maker budget constraints, the welfare-maximizing matching problem is a plain Linear Program. This is the structural insight that makes Sybil tractable: the core problem is trivially solvable, and all computational difficulty comes from a small number of [[MM Budget Constraint|bilinear side constraints]].

The LP has three kinds of decision variables: fill quantities `q_i` in fixed-point share-units (how much of each order to fill, bounded by `[0, max_fill]`), per-market minting `mint_m`, and group minting `gmint_g`. The objective is [[Welfare Maximization|total welfare]]: `sum(L_i * q_i / SHARE_SCALE)` for buyers minus `sum(L_j * q_j / SHARE_SCALE)` for sellers minus minting costs. The constraints are position balance (for each market and outcome, total demand cannot exceed total supply plus minting), quantity bounds, and non-negativity of minting variables. That's it — a textbook LP.

The problem size scales as O(N + M + G) variables and O(N + M) constraints, where N is the number of orders, M the number of markets, and G the number of groups. For a typical batch of 10,000 orders across 100 markets and 10 groups, modern LP solvers (HiGHS, used by the [[LP Solver]]) solve this in under a millisecond. The dual variables of the position balance constraints are the [[LP Duality and Clearing Prices|clearing prices]], and all economic properties (uniform clearing prices, price normalization, group consistency) emerge automatically from LP duality. No post-hoc enforcement is needed.

## Key Properties
- Variables: `q_i` (fills), `mint_m` (per-market), `gmint_g` (group) — all continuous, bounded
- Constraints: position balance per market per outcome + quantity bounds
- O(N + M + G) size — trivially solvable by simplex or interior-point methods
- Clearing prices = [[LP Duality and Clearing Prices|dual variables]] of balance constraints
- The [[MM Budget Constraint]] is the only thing that makes this hard

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — LP construction and solving via HiGHS
> `design/problem-statement.md` — formal boxed LP formulation (Section 7)

## See Also
- [[MM Budget Constraint]] — the bilinear coupling that makes the full problem NP-hard
- [[LP Duality and Clearing Prices]] — how prices emerge from the LP dual
- [[Welfare Maximization]] — the linear objective function
- [[Minting]] — minting variables in the LP
