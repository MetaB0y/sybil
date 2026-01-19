# Sybil V2: Architecture

## Overview

Sybil V2 is a prediction market matching engine using Frequent Batch Auctions (FBA) with:
- **Linear constraint orders** - Orders are payoff vectors over market outcomes
- **Two-phase solving** - Per-market clearing, then cross-market optimization
- **Uniform clearing price (UCP)** - All fills in a market execute at the same price

## Two-Phase Architecture

```
┌─────────────────────────────────────────────────────────┐
│ Phase 1: Per-Market Clearing                            │
│                                                         │
│   Input: Orders for each market                         │
│   Method: Find clearing prices where Σp_i = 1           │
│   Output: Prices + fills per market                     │
│                                                         │
│   Solver: LocalSolver (src/local_solver.rs)             │
└─────────────────────────────────────────────────────────┘
                          ↓
              Clearing prices + initial fills
                          ↓
┌─────────────────────────────────────────────────────────┐
│ Phase 2: Cross-Market Optimization                      │
│                                                         │
│   A. MM Budget Allocation                               │
│      - Which MM orders to activate given budgets        │
│      - Lagrangian relaxation + fixed-point iteration    │
│      - Solver: MmAllocator (src/mm_allocator.rs)        │
│                                                         │
│   B. Cross-Market Patches (optional)                    │
│      - Bundle orders, spreads, arbitrage                │
│      - Combine via MWIS on conflict graph               │
│      - Solver: Combiner (src/combiner/)                 │
└─────────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Linear Constraint Orders

Orders are expressed as payoff vectors over outcomes:
```rust
struct Order {
    markets: [MarketId; MAX_MARKETS],
    payoffs: [i8; MAX_STATES],    // Payoff per state
    limit_price: Nanos,           // Max willing to pay
    min_fill: Qty,
    max_fill: Qty,
}
```

This supports: simple limits, spreads, bundles, butterflies, conditionals.

### 2. Price Normalization

For multi-outcome markets, prices must satisfy Σp_i = 1 (no-arbitrage).
Buying one share of each outcome costs exactly $1.

### 3. Uniform Clearing Price (UCP)

All fills in a market execute at the same price:
- No front-running (batch ordering doesn't matter)
- Price = supply/demand equilibrium
- Welfare = Σ (limit - price) × quantity for buyers

### 4. MM Budget Constraints

Market makers have capital budgets spanning multiple markets:
```
Capital needed = f(price, quantity, side)
  - Selling YES: (1 - price) × qty
  - Buying YES: price × qty
```

The budget constraint Σ capital_i ≤ K is bilinear in (price, quantity).

**Solution:** Two-phase with Lagrangian relaxation:
1. Get prices from Phase 1
2. Binary search on λ to find which orders to activate

---

## Module Structure

```
crates/
├── matching-engine/     # Core types
│   ├── order.rs         # Order representation
│   ├── fill.rs          # Fill execution
│   ├── liquidity.rs     # Liquidity pools
│   ├── problem.rs       # Problem definition
│   ├── market.rs        # Market definitions
│   └── mm.rs            # MM constraints
│
├── matching-solver/     # Solving algorithms
│   ├── local_solver.rs  # Per-market clearing
│   ├── mm_allocator.rs  # MM budget allocation
│   ├── combiner/        # Solution combination
│   │   ├── conflict.rs  # Conflict graph
│   │   └── mwis.rs      # MWIS algorithms
│   └── specialized/     # Specialized solvers
│
├── matching-scenarios/  # Test scenarios
│   ├── mega.rs          # Mega scenario generator
│   ├── random.rs        # Random generation
│   └── stress.rs        # Stress testing
│
└── matching-sim/        # CLI tool
    └── main.rs
```

---

## Complexity Analysis

| Resource | Practical Limit | Determined By |
|----------|-----------------|---------------|
| Orders per batch | ~50K | Solver capacity |
| Markets per batch | ~1K | State management |
| MMs per batch | ~10 | Fixed-point iterations |

---

## Solver Types

### LocalSolver (Per-Market)
- Finds clearing prices for multi-outcome markets
- Ensures Σp_i = 1 (normalization)
- O(n × m) for n orders, m outcomes

### MmAllocator (MM Constraints)
- Binary search on λ per MM
- Fixed-point iteration for multiple interacting MMs
- Respects budget constraints at clearing prices

### Specialized Solvers (Cross-Market)
| Solver | Purpose |
|--------|---------|
| Arbitrage | Find cross-market mispricings |
| BundleDecomposer | Fill bundle orders |
| ChainFinder | Exploit implication chains |

---

## Welfare Calculation

```
Welfare = Σ (limit_price - clearing_price) × fill_qty
```

For buyers: value received - price paid
For sellers: price received - cost

---

## Solver Ordering Analysis

### The Problem

MM orders can represent significant volume (potentially 10x retail orders). The question is whether MM volume should affect clearing prices.

**Current approach (prices first, then MM)**:
```
1. LocalSolver on non-MM orders → prices
2. MmAllocator uses those prices → activated MM orders
3. Done
```

**Issue**: If MM provides 90% of liquidity, Phase 1 prices could be way off.

### Three Options

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| 1. After | Per-market clearing first, then MM allocation | Simple | Prices may be wrong if MM dominates |
| 2. Include MM | Include MM orders in clearing | MM affects prices | Can't enforce budget (circular) |
| 3. Iterative | Fixed-point between clearing and allocation | Correct | More complex, slower |

### Recommendation: Option 3 (Iterative)

```
1. LocalSolver on non-MM orders → prices_1
2. MmAllocator(prices_1) → activated_mm_1
3. LocalSolver on (non-MM + activated_mm_1) → prices_2
4. If prices_2 ≈ prices_1: done
5. MmAllocator(prices_2) → activated_mm_2
6. Repeat until convergence (typically 1-3 iterations)
```

**Why this works**:
- Prices reflect actual supply (including activated MM orders)
- MM budgets are respected at the prices that include their orders
- Convergence is fast because price changes are dampened by MM budget limits

**When to use Option 1 instead**:
- MM volume is small (< 20% of total)
- Speed is critical and accuracy can be sacrificed
- Testing/debugging (simpler to reason about)

### Implementation Status

Current: **Option 1** (per-market first, then MM allocation)

Next step: Implement Option 3 with convergence detection and benchmark the welfare difference.

---

## References

- [Frequent Batch Auctions (Budish et al.)](https://faculty.chicagobooth.edu/eric.budish/research/HFT-FrequentBatchAuctions.pdf)
- [Maximum Weight Independent Set](https://en.wikipedia.org/wiki/Independent_set_(graph_theory))
