---
tags: [concept, economics]
layer: solver
status: current
last_verified: 2026-03-15
---

Clearing prices in Sybil are not set by a pricing algorithm — they emerge naturally as dual variables of the [[The LP Core|LP's]] position balance constraints. This is one of the most elegant aspects of the formulation: the economics of price discovery are a free consequence of solving the welfare optimization problem.

Every LP has a dual. The dual variable associated with a constraint represents the marginal value of relaxing that constraint by one unit. For the position balance constraint on market m, outcome o — "total demand cannot exceed total supply plus minting" — the dual variable is exactly the clearing price for that outcome. If you could magically create one more unit of supply, the welfare would increase by exactly the clearing price. This is the textbook definition of a competitive market price.

LP duality gives three economic properties for free. First, the Uniform Clearing Price (UCP) condition: complementary slackness says that if an order fills (`q_i > 0`), its surplus must be non-negative — buyers only fill if their limit is at or above the clearing price, sellers only fill if their limit is at or below. Second, price normalization: the stationarity condition on the per-market minting variable gives `YES_price + NO_price <= $1`, with equality when [[Minting]] is active (which it almost always is). Third, group consistency: the stationarity condition on group minting gives `sum(YES_prices) <= $1` across markets in a [[Binary Markets and Market Groups|group]], with equality when group minting is active. All three conditions are enforced automatically by the solver — no post-hoc price adjustment is needed.

## Key Properties
- Clearing price = dual variable of position balance constraint
- Complementary slackness = Uniform Clearing Price (UCP)
- [[Minting]] stationarity = `YES + NO <= $1` per market
- Group minting stationarity = `sum(YES) <= $1` per [[Binary Markets and Market Groups|group]]
- All economic constraints emerge from LP duality — zero enforcement code needed

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — dual variable extraction after solve
> `design/problem-statement.md` — dual conditions table (Section 7)

## See Also
- [[The LP Core]] — the primal LP whose dual gives prices
- [[Welfare Maximization]] — total welfare is independent of prices (depends only on fills)
- [[Minting]] — price normalization through minting stationarity
