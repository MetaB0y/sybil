---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-03-15
---

The solver maximizes total welfare — the sum of consumer surplus across all filled orders. For a buyer willing to pay 60 cents who gets filled at a 50-cent clearing price, the surplus is 10 cents per share. The objective function is `W = sum(limit_price * qty)` for buyers minus `sum(limit_price * qty)` for sellers minus minting costs. A standard result in auction theory shows that total welfare depends only on which orders fill and how much, not on the clearing price itself. The price determines who captures the surplus (buyers vs sellers) but not the total amount created.

This is a deliberate design choice with real consequences. A zero-surplus order — someone willing to buy at exactly the clearing price — may not fill if higher-surplus orders consume all available liquidity. The solver is not trying to maximize the number of trades; it's trying to maximize the total value created. See [[Welfare vs Volume]] for the full analysis of welfare vs volume tradeoffs, including allocative efficiency arguments, the impact on price discovery, and when volume maximization might be preferable.

## Key Properties
- Objective: `W = sum_buyers(L_i * q_i) - sum_sellers(L_j * q_j) - $1 * sum(mint) - $1 * sum(gmint)`
- Linear in fill quantities — the clearing price doesn't appear in the objective
- Higher-surplus orders are naturally preferred by the optimizer
- Zero-surplus orders get lowest priority and may not fill
- [[Minting]] costs appear as negative terms to prevent unbounded share creation

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — LP objective function construction
> `design/problem-statement.md` — formal mathematical definition

## See Also
- [[Welfare vs Volume]] — arguments for and against each objective
- [[The LP Core]] — the linear program that implements this objective
- [[LP Duality and Clearing Prices]] — how prices emerge from the welfare optimization
