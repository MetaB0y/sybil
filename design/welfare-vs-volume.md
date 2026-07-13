---
tags: [matching, economics, reference]
status: reference
last_verified: 2026-07-11
---

# Welfare vs Volume: Analysis of the Matching Solver

This document analyzes how the matching solver balances welfare maximization against volume maximization.

## Executive Summary

The solver is **strongly biased toward welfare maximization**. While it doesn't explicitly reject zero-surplus trades, it:

1. Prioritizes high-surplus trades when resources are scarce
2. Uses welfare-based ordering throughout all solver stages
3. May leave volume on the table to capture higher welfare

This is economically sound for most exchanges but may not be optimal for all use cases.

---

## What We Maximize

### Welfare Definition

Welfare is defined as the sum of consumer surplus across all fills:

```
Total Welfare = Σ (limit_price - fill_price) × fill_qty
```

For a buyer willing to pay 60¢ who gets filled at 50¢:
- Surplus per unit = 60¢ - 50¢ = 10¢
- If they buy 100 shares: welfare = $10

### Volume Definition

Volume is simply total filled quantity:

```
Total Volume = Σ fill_qty
```

### The Tradeoff

The question is: **would we ever sacrifice $10 of volume for $0.10 of welfare?**

---

## How the Solver Enforces Welfare-First

The LP baseline (`lp_solver.rs`) maximizes `Σ (limit_price - clearing_price) × fill_qty` directly as its LP objective. This means:

- Orders with higher surplus are naturally preferred by the optimizer
- Zero-surplus orders (limit = clearing price) may not fill if they compete with higher-surplus orders for liquidity
- MM budget allocation uses iterative shading (SLP) which also prioritizes welfare per unit of capital

**Example**:
```
Clearing price: 50¢, Liquidity: 100 shares
Orders:
  A: limit 70¢, wants 50 shares (welfare/share = 20¢)
  B: limit 60¢, wants 50 shares (welfare/share = 10¢)
  C: limit 50¢, wants 50 shares (welfare/share = 0¢)

Result:
  A fills 50 shares (welfare = $10)
  B fills 50 shares (welfare = $5)
  C fills 0 shares ← SACRIFICED VOLUME

Total: 100 shares, $15 welfare
```

Order C could trade at the clearing price but doesn't get filled because higher-welfare orders consume all liquidity. **This sacrifices 50 shares of potential volume to maximize welfare.**

The production `RetainedCashSolver` and `ConicSolver` in QuasiFisher mode use
the paper's affine-to-log MM objective. This intentionally differs from pure
risk-neutral welfare only when shared MM capital binds; the paper supplies the
corresponding welfare bound. See `paper.typ` in
`~/github/prediction-markets-are-fisher-markets/` (pointer
`design/math-papers.md`) for the theoretical foundation.

---

## Quantifying the Tradeoff

### Current Behavior

The LP solver's welfare objective naturally prioritizes high-surplus orders. Zero-surplus orders get lowest priority and may not fill when liquidity is scarce.

### Specific Scenarios

**Scenario 1: Would we sacrifice $10 volume for $0.10 welfare?**

Yes, this can happen:

```
Clearing price: 50¢, Liquidity: 100 shares
Order A: limit 50.001¢, wants 100 shares (welfare = $0.001 × 100 = $0.10)
Order B: limit 50.000¢, wants 100 shares (welfare = $0.000)

Result: A fills all 100 shares, B fills nothing
Welfare gained: $0.10
Volume sacrificed: 100 shares (nominal value ~$50)
```

**Scenario 2: Would we sacrifice $100 volume for $1 welfare?**

Yes:

```
Clearing price: 50¢, Liquidity: 100 shares
Order A: limit 51¢, wants 100 shares
Order B: limit 50¢, wants 100 shares

A fills 100 at 50¢ (welfare = $1)
B fills nothing
Volume potential was 200 shares, we filled 100
```

**Scenario 3: When does welfare-ordering help volume?**

When the welfare order happens to match capacity:

```
Liquidity: 100 shares
Order A: limit 70¢, wants 50 shares (welfare potential = $10)
Order B: limit 60¢, wants 50 shares (welfare potential = $5)

Both fill perfectly, total 100 shares, $7.50 welfare
If FIFO with A first: same result
If FIFO with B first: B gets 50, A gets 50, same volume, same welfare
```

---

## Is This Economically Sound?

### Arguments FOR welfare maximization:

1. **Allocative efficiency**: Resources go to those who value them most
2. **Price discovery**: Higher limits signal stronger conviction, better information
3. **Incentive compatibility**: Traders are incentivized to bid true valuations
4. **Market health**: Prevents spam orders at marginal prices

### Arguments FOR volume maximization:

1. **Exchange function**: If someone sells at 50¢ and someone buys at 50¢, the exchange fulfilled its purpose
2. **Liquidity externalities**: More trades → more data → better prices for everyone
3. **Fee revenue**: Exchanges are paid per trade, not per welfare unit
4. **User satisfaction**: Users want their orders filled, regardless of surplus

### The Right Answer

There is no universally "correct" answer. It depends on exchange goals:

- **Prediction markets**: Welfare maximization is standard (Budish-Cramton-Shim 2015)
- **Traditional exchanges**: Often FIFO for fairness, regulatory compliance
- **DEXs/AMMs**: Volume-maximizing to minimize slippage
- **Auctions**: Welfare-maximizing is standard (Vickrey 1961)

---

## Recommendations

### Option 1: Keep Current (Welfare-First)

**Pros**: Economically efficient, good for informed traders
**Cons**: May frustrate users with zero-surplus orders

### Option 2: Hybrid Approach

Add a configurable threshold:

```rust
// Proposed: Fill zero-surplus orders if welfare loss is bounded
const MAX_WELFARE_SACRIFICE_FOR_VOLUME: f64 = 0.001; // 0.1% of total

if remaining_liquidity > 0 {
    // Fill zero-surplus orders up to threshold
    for order in zero_surplus_orders {
        if marginal_welfare_loss() < MAX_WELFARE_SACRIFICE_FOR_VOLUME * total_welfare {
            fill(order);
        }
    }
}
```

### Option 3: Volume Maximization Mode

Add a solver mode that maximizes volume:

```rust
pub enum OptimizationObjective {
    MaxWelfare,    // Current behavior
    MaxVolume,     // Fill as many orders as possible
    Hybrid(f64),   // Weighted combination
}
```

### Option 4: Tiered Priority

```
Priority 1: High-welfare orders (surplus > 1%)
Priority 2: Medium-welfare orders (surplus 0.1% - 1%)
Priority 3: Zero-surplus orders (surplus < 0.1%)
```

This ensures zero-surplus orders still fill when capacity allows, but don't crowd out valuable trades.

---

## Academic References

1. **Vickrey (1961)**: "Counterspeculation, Auctions, and Competitive Sealed Tenders" - Foundation for welfare-maximizing mechanisms

2. **Myerson-Satterthwaite (1983)**: "Efficient Mechanisms for Bilateral Trading" - Shows impossibility of full efficiency with budget balance

3. **Budish-Cramton-Shim (2015)**: "The High-Frequency Trading Arms Race" - Proposes frequent batch auctions for equity markets

4. **Roughgarden (2016)**: "Twenty Lectures on Algorithmic Game Theory" - Survey of mechanism design

---

## Conclusion

The current implementation strongly favors welfare over volume. This is a deliberate design choice that:

1. **Benefits**: Ensures allocative efficiency, rewards informed traders
2. **Costs**: May leave volume on the table, frustrate marginal traders

Multi-market orders (bundles, spreads) are handled natively by the LP/EG/Conic
solvers as payoff vectors over joint market states. The welfare-first bias
applies uniformly to all order types.

For most prediction market use cases, welfare maximization is appropriate. If
volume maximization becomes important (e.g., for fee revenue or liquidity
depth), consider implementing a hybrid mode.
