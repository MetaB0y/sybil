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

## Component-by-Component Analysis

### 1. LocalSolver (Per-Market Clearing)

**File**: `local_solver.rs`

**How it works**:
1. Finds clearing price via supply-demand crossing (lines 408-491)
2. At that price, fills buyers sorted by **welfare contribution** (lines 228-229)

**Key code** (line 228-229):
```rust
// Sort buyers by welfare contribution (descending)
buyers.sort_by(|a, b| b.2.cmp(&a.2));
```

**Welfare vs Volume behavior**:
- The clearing price mechanism inherently finds the price that maximizes volume at that price level
- BUT when filling at that price, orders are filled by welfare priority, not FIFO
- Zero-surplus orders (limit = clearing price) get **lowest priority**
- If liquidity is limited, zero-surplus orders may not fill at all

**Example**:
```
Clearing price: 50¢
Liquidity: 100 shares available
Orders:
  A: limit 70¢, wants 50 shares (welfare/share = 20¢)
  B: limit 60¢, wants 50 shares (welfare/share = 10¢)
  C: limit 50¢, wants 50 shares (welfare/share = 0¢)

Result:
  A fills 50 shares (welfare = $10)
  B fills 50 shares (welfare = $5)
  C fills 0 shares ← SACRIFICED VOLUME

Total: 100 shares, $15 welfare
Alternative (FIFO): 100 shares, $10-15 welfare (depending on order arrival)
```

**Verdict**: Order C (zero-surplus) could trade at the clearing price, but doesn't get filled because higher-welfare orders consume all liquidity. **This sacrifices 50 shares of potential volume to maximize welfare.**

### 2. GreedySolver

**File**: `greedy.rs`

**How it works**:
Processes orders in decreasing order of **welfare potential**:

```rust
// Lines 39-48
fn sort_by_welfare(orders: &[Order]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..orders.len()).collect();
    indices.sort_by(|&a, &b| {
        let welfare_a = orders[a].limit_price as u128 * orders[a].max_fill as u128;
        welfare_b.cmp(&welfare_a)  // Descending
    });
    indices
}
```

**Welfare vs Volume behavior**:
- Orders are processed strictly by `limit_price × max_fill`
- An order with high limit but small quantity goes before a low-limit high-quantity order
- This can leave substantial volume unfilled

**Example**:
```
Liquidity: 100 shares at 50¢
Orders:
  A: limit 90¢, wants 10 shares (potential = $9)
  B: limit 51¢, wants 100 shares (potential = $51)

Processing order (by potential):
  1. B: fills 100 shares at 50¢ (welfare = $1)
  2. A: no liquidity left, unfilled

Result: 100 shares, $1 welfare
```

In this case, the greedy welfare sort actually helps volume because the high-volume order happens to have higher total potential. But if:

```
Orders:
  A: limit 90¢, wants 100 shares (potential = $90)
  B: limit 51¢, wants 10 shares (potential = $5.10)

Processing:
  1. A: fills 100 shares (welfare = $40)
  2. B: unfilled

Result: 100 shares, $40 welfare
```

**Verdict**: The greedy approach maximizes total welfare potential, which often correlates with volume but not always.

### 3. MmAllocator (Market Maker Budget Allocation)

**File**: `mm_allocator.rs`

**How it works**:
Allocates constrained MM budgets by **welfare/capital ratio**:

```rust
// Lines 274-278
order_info.sort_by(|(_, w1, c1), (_, w2, c2)| {
    let ratio1 = if *c1 > 0 { *w1 as f64 / *c1 as f64 } else { f64::MAX };
    let ratio2 = if *c2 > 0 { *w2 as f64 / *c2 as f64 } else { f64::MAX };
    ratio2.partial_cmp(&ratio1).unwrap_or(std::cmp::Ordering::Equal)
});
```

**Welfare vs Volume behavior**:
- Orders that generate more welfare per dollar of capital go first
- Zero-welfare orders (limit = fill price) have ratio = 0 and get lowest priority
- This is a **capital efficiency** measure, not pure welfare or volume

**Example**:
```
MM budget: $100
Orders:
  A: needs $50 capital, generates $5 welfare (ratio = 0.10)
  B: needs $100 capital, generates $2 welfare (ratio = 0.02)
  C: needs $50 capital, generates $0 welfare (ratio = 0.00)

Allocation:
  1. A activates ($50 used, $5 welfare)
  2. C next in remaining budget... but $0 welfare
  3. Budget allows C (50 shares) OR part of B

Current implementation: activates C (it has ratio = 0, but still > no fill)
Actually: C gets activated because ratio sort puts it last but budget allows it
```

**Verdict**: The MM allocator explicitly prioritizes welfare/capital ratio. Zero-surplus trades can still happen if budget allows after high-ratio orders.

### 4. ArbitrageDetector (Bundle Matching)

**File**: `arbitrage.rs`

**How it works**:
1. Detects cross-market opportunities
2. Sorts by **profit per unit** (line 79-80)
3. Only adds fills with **welfare > 0** (line 291-294)

```rust
// Lines 291-294
if let Some(fill) = fill_result {
    let welfare = fill.welfare(order);
    if welfare > 0 {  // ← Explicit welfare requirement!
        result.add_fill(fill, order);
        filled_orders.insert(order.id);
    }
}
```

**Welfare vs Volume behavior**:
- **Explicitly rejects zero-welfare fills** (line 291)
- This is the only component that hard-rejects zero-surplus trades

**Verdict**: ArbitrageDetector will never fill a bundle order at exactly its limit price. This sacrifices volume for welfare guarantees.

---

## Quantifying the Tradeoff

### Current Behavior

Based on the code analysis:

| Component | Zero-surplus handling | Tradeoff severity |
|-----------|----------------------|-------------------|
| LocalSolver | Lowest priority, may not fill | Medium |
| GreedySolver | Processed last by welfare sort | Medium |
| MmAllocator | Lowest priority by ratio | Low |
| ArbitrageDetector | **Explicitly rejected** | High |

### Specific Scenarios

**Scenario 1: Would we sacrifice $10 volume for $0.10 welfare?**

Yes, this can happen in LocalSolver:

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

The most significant issue is in `ArbitrageDetector` which **explicitly rejects** zero-welfare bundle fills. This could be relaxed to allow zero-surplus fills when there's no competing higher-surplus order.

For most prediction market use cases, welfare maximization is appropriate. If volume maximization becomes important (e.g., for fee revenue or liquidity depth), consider implementing a hybrid mode or relaxing the zero-welfare rejection in ArbitrageDetector.
