---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-07-13
---

The solver maximizes total welfare — the sum of consumer surplus across all filled orders. For a buyer willing to pay 60 cents who gets filled at a 50-cent clearing price, the surplus is 10 cents per share. With fixed-point share quantities, the objective is `W = sum(limit_price * qty_units / SHARE_SCALE)` for buyers minus `sum(limit_price * qty_units / SHARE_SCALE)` for sellers minus signed complete-set cost. A standard result in auction theory shows that total welfare depends only on which orders fill and how much, not on the clearing price itself. The price determines who captures the surplus (buyers vs sellers) but not the total amount created.

The signed complete-set term follows the zero-temperature Fisher-market formulation: `V(D) = max_state D_state`. Creating a complete set has `V(D) > 0` and consumes collateral; burning a complete set has `V(D) < 0` and releases collateral. Therefore burning must be credited by subtracting a negative cost. In landed settlement this term is exactly the negation of the real participants' aggregate fill balance delta. Clamping it to zero for burns understates welfare by the released collateral and can produce impossible negative totals. For verifier-clean uniform-price fills, net welfare equals the sum of nonnegative per-fill consumer surplus.

The off-block all-time/24h tracker records only verified non-negative block
totals. On restore, negative legacy aggregates created before the signed-burn
fix are clamped to zero; they are not protocol state and cannot be repaired
exactly from an already-aggregated historical scalar.

This is a deliberate design choice with real consequences. A zero-surplus order — someone willing to buy at exactly the clearing price — may not fill if higher-surplus orders consume all available liquidity. The solver is not trying to maximize the number of trades; it's trying to maximize the total value created. See [[Welfare vs Volume]] for the full analysis of welfare vs volume tradeoffs, including allocative efficiency arguments, the impact on price discovery, and when volume maximization might be preferable.

## Key Properties
- Objective: `W = sum_buyers(L_i * q_i / SHARE_SCALE) - sum_sellers(L_j * q_j / SHARE_SCALE) - V(D)`, where `V(D)` is signed
- Linear in fill quantities — the clearing price doesn't appear in the objective
- Higher-surplus orders are naturally preferred by the optimizer
- Zero-surplus orders get lowest priority and may not fill
- Complete-set creation is a positive cost; complete-set burning is a negative cost (a collateral-release credit)

## Where This Lives
> `crates/matching-solver/src/lp_solver.rs` — LP objective function construction
> `design/problem-statement.md` — formal mathematical definition

## See Also
- [[Welfare vs Volume]] — arguments for and against each objective
- [[The LP Core]] — the linear program that implements this objective
- [[LP Duality and Clearing Prices]] — how prices emerge from the welfare optimization
