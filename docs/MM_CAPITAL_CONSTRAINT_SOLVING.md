# MM Flash Liquidity: Architecture and Integration

## The Core Problem

Market makers (MMs) want to provide liquidity across many markets with limited capital:

```
MM has capital K = $10,000
MM wants to quote on N = 100 markets
Most markets won't trade in any batch
Locking K/N = $100 per market is useless
```

**Flash Liquidity Goal:** MM commits conditionally across all markets. Actual capital usage determined at clearing time, never exceeding K.

This is NOT leverage - MM never owes more than they have. It's deferred allocation.

---

## Why This Is Hard: The Bilinearity

For prediction markets, capital needed depends on clearing price:

```
Selling YES: capital = (1 - price) × quantity
Buying YES:  capital = price × quantity
```

The constraint `Σ capital_i ≤ K` contains terms `price × quantity`.

**The circularity:**
```
Prices ← depend on ← MM supply (how much MM fills)
   ↓                      ↑
   └───→ determines → capital needed → determines → MM fills
```

Both price and quantity are unknowns → bilinear constraint → hard optimization.

---

## Key Insight: MM Constraints Are Cross-Market

The capital constraint spans multiple markets. This is structurally similar to other cross-market orders:

| Order Type | Constraint |
|------------|-----------|
| Simple limit | Single market, single price bound |
| Jumbo | Multiple markets, all-or-nothing |
| MM constrained | Multiple markets, budget constraint |

**All cross-market constraints belong to the same architectural layer.**

---

## Two-Phase Architecture

```
┌─────────────────────────────────────────────────────────┐
│ Phase 1: Per-Market Clearing                            │
│                                                         │
│   Input: Simple orders only                             │
│   Method: Optimal per-market matching (tractable)       │
│   Output: Base clearing prices P_base                   │
│                                                         │
│   Note: Cross-market orders (jumbos, MM) not included   │
└─────────────────────────────────────────────────────────┘
                          ↓
              Base prices (anchor, not final)
                          ↓
┌─────────────────────────────────────────────────────────┐
│ Phase 2: Cross-Market Optimization                      │
│                                                         │
│   Input:                                                │
│     - Order book (all orders)                           │
│     - Cross-market constraints (jumbos, MM budgets)     │
│     - Base prices P_base (hint)                         │
│                                                         │
│   Solvers: Propose matchings (who fills, how much)      │
│   Platform: Validates proposals, combines via MWIS      │
│   Output: Final matching and prices                     │
└─────────────────────────────────────────────────────────┘
```

**Why two phases?**
- Phase 1 breaks the circularity (prices first, then cross-market)
- Phase 2 handles the hard cross-market optimization
- Solvers compete on Phase 2 (the valuable/hard part)

---

## Solver Proposals: Matchings, Not Prices

**Key design decision:** Solvers propose matchings (fills), not prices.

```
Solver output:
  - "Fill order X with quantity Q"
  - "Fill MM orders: 150 on market A, 100 on market B"
  - "Fill jumbo J"

NOT:
  - "Clear market A at price $0.50"
```

**Platform computes prices** from the aggregate matching (supply/demand intersection).

**Platform validates:**
1. Compute implied prices from proposed matching
2. Check all limit orders satisfied at implied prices
3. Check all cross-market constraints (jumbos, MM budgets)
4. Reject invalid proposals

---

## MM Budget Validation

Given a proposed matching that includes MM fills:

```python
def validate_mm_constraint(matching, mm_constraint):
    # Compute implied prices from full matching
    prices = compute_clearing_prices(matching)

    # Calculate capital used at implied prices
    capital_used = 0
    for market_id, fill_qty in matching.mm_fills[mm_constraint.mm_id]:
        price = prices[market_id]
        if mm_constraint.side[market_id] == SELL_YES:
            capital_used += (1 - price) * fill_qty
        else:
            capital_used += price * fill_qty

    # Check constraint
    if capital_used > mm_constraint.max_capital:
        return Invalid(f"MM budget exceeded: {capital_used} > {mm_constraint.max_capital}")

    # Check limit prices
    for market_id, fill_qty in matching.mm_fills[mm_constraint.mm_id]:
        if prices[market_id] < mm_constraint.limit_prices[market_id]:
            return Invalid(f"MM limit price violated on market {market_id}")

    return Valid()
```

**The solver's job:** Find a matching that passes this validation.

**The platform's job:** Validate and combine proposals.

---

## Multiple MMs

With multiple MMs, each has independent budget constraint:

```
MM1: Budget K1, orders on markets {A, B, C}
MM2: Budget K2, orders on markets {B, C, D}
MM3: Budget K3, orders on markets {A, D, E}
```

**Validation:** Check each MM's constraint independently.

**Interaction:** MMs affect each other through prices:
- MM1 fills on B → changes price on B → affects MM2's capital cost on B

**Solver must find:** A matching where ALL MM constraints are satisfied at the implied prices.

### Multi-MM Complexity

| Scenario | Complexity |
|----------|------------|
| Single MM | Solver finds allocation satisfying one constraint |
| Multiple MMs, non-overlapping markets | Independent, easy |
| Multiple MMs, overlapping markets | Coupled through prices, harder |
| Multiple MMs competing for same liquidity | Need to allocate, hardest |

**For overlapping MMs:** Solver must simultaneously satisfy:
- MM1's budget at prices implied by (MM1 + MM2 + ... fills)
- MM2's budget at those same prices
- etc.

This is a fixed-point problem, but solvers can use any method:
- Lagrangian relaxation (one λ per MM)
- Iterative adjustment
- Direct optimization
- Heuristics

---

## MWIS Integration

The combiner (MWIS) handles cross-market proposals.

### Conflict Types

```rust
enum Conflict {
    SameOrder,         // Both proposals fill same order differently
    LiquidityExceeded, // Combined fills exceed available liquidity
    PriceInconsistent, // Proposals imply different prices for same market
    ConstraintViolated,// Combined proposals violate a cross-market constraint
}
```

### Price Consistency

**Critical issue:** Two proposals might each be valid, but imply different prices.

```
Proposal A: MM1 fills heavily on market X → price drops to $0.48
Proposal B: MM2 fills lightly on market X → price stays at $0.52

Cannot combine: would need two prices for market X.
```

**Solution:** Proposals that imply different prices for the same market are conflicting.

### Constraint Consistency

Two proposals might each satisfy MM constraints individually, but violate when combined:

```
Proposal A: MM1 fills on {A, B}, uses $800 of $1000 budget ✓
Proposal B: MM1 fills on {C, D}, uses $400 of $1000 budget ✓
Combined: MM1 uses $1200 > $1000 ✗
```

**Solution:** If both proposals touch the same MM's orders, check combined constraint.

---

## Practical Implications

### What Combines Well

| Scenario | Combinability |
|----------|---------------|
| Proposals on disjoint markets | High (independent) |
| Proposals touching same market, similar fills | Medium (if prices close) |
| Proposals with different MM allocations | Low (price inconsistency) |
| Same MM in multiple proposals | Very low (budget conflict) |

### Expected Behavior

For cross-market orders (jumbos, MM constraints):
- **MWIS mostly selects ONE proposal**, not combines many
- Competition is about finding the BEST cross-market solution
- Combining adds value for independent improvements

This is similar to Ethereum block building:
- Many bundles conflict
- Builder picks best non-conflicting set
- Value comes from competition, not just combination

---

## Solving Strategies for Solvers

Solvers need to find matchings that satisfy MM constraints at implied prices.

### Strategy 1: Lagrangian Relaxation

For each MM, introduce shadow price λ for capital:

```
Effective limit price = original_limit + λ × capital_cost_per_unit
```

Binary search on λ to find where capital usage = K.

**For multiple MMs:** One λ per MM, iterate until all constraints satisfied.

### Strategy 2: Fixed-Point Iteration

```
1. Start with base prices
2. Compute MM fills respecting budgets at current prices
3. Recompute prices with these fills
4. Check if budgets still satisfied
5. Repeat until convergence
```

**Damping helps:** `prices_new = α × computed + (1-α) × old`

### Strategy 3: Direct Optimization

Formulate as constrained optimization:
- Variables: fill quantities
- Objective: welfare
- Constraints: budget constraints (bilinear), limit prices

Use numerical solver (SLSQP, interior point, etc.)

### Which Strategy?

| Strategy | Pros | Cons |
|----------|------|------|
| Lagrangian | Principled, interpretable | Multiple λ iteration for multi-MM |
| Fixed-point | Intuitive | No convergence guarantee |
| Direct optimization | General | Needs library, slower |

**Recommendation:** Start with Lagrangian for single MM, fixed-point for multi-MM.

---

## Validation Plan

### Open Questions

1. **Does MWIS combining work?** Or do most proposals conflict on prices?
2. **How often do MM constraints bind?** If rarely, architecture is overkill.
3. **Do solvers find good solutions?** Or is the problem too hard?
4. **Price consistency tolerance?** Can we allow small price differences?

### Simulation Scenarios

```rust
struct TestScenario {
    num_markets: usize,
    num_simple_orders: usize,
    num_jumbos: usize,
    num_mms: usize,
    mm_budget_tightness: f64,  // 0.0 = slack, 1.0 = very tight
    mm_market_overlap: f64,    // 0.0 = disjoint, 1.0 = all same markets
}

// Key scenarios to test:
let scenarios = vec![
    // Baseline: no cross-market
    TestScenario { num_mms: 0, num_jumbos: 0, .. },

    // Single MM, slack budget
    TestScenario { num_mms: 1, mm_budget_tightness: 0.3, .. },

    // Single MM, tight budget
    TestScenario { num_mms: 1, mm_budget_tightness: 0.9, .. },

    // Multiple MMs, non-overlapping
    TestScenario { num_mms: 3, mm_market_overlap: 0.0, .. },

    // Multiple MMs, overlapping
    TestScenario { num_mms: 3, mm_market_overlap: 0.5, .. },

    // MMs + jumbos
    TestScenario { num_mms: 2, num_jumbos: 5, .. },
];
```

### Metrics to Track

1. **Welfare achieved** vs theoretical optimum
2. **Constraint satisfaction** rate
3. **Proposal validity** rate (what fraction pass validation)
4. **Combining rate** (how many proposals combine vs. single winner)
5. **Price deviation** from optimal
6. **Solver runtime**

### Implementation Steps

1. **Fix combiner issues** (multi-market outcome detection, constraint validation)
2. **Add MM constraint order type** to matching engine
3. **Add MM constraint validation** to combiner
4. **Create test scenarios** with MM constraints
5. **Run simulations** and measure metrics
6. **Iterate** based on findings

---

## Summary

| Aspect | Design Decision |
|--------|-----------------|
| MM constraints | Cross-market, handled in Phase 2 with jumbos |
| Solver output | Matchings (fills), not prices |
| Price discovery | Platform computes from matching |
| Validation | Platform checks constraints at implied prices |
| Multi-MM | Independent constraints, coupled through prices |
| MWIS | Conflicts include price inconsistency |
| Expectation | Limited combining for cross-market; value from competition |

**Key uncertainty:** Does this work in practice? Need simulation to validate.

---

## Appendix: Capital Cost Formulas

For prediction markets with $1 settlement:

```rust
fn capital_cost(side: Side, price: Price, quantity: Qty) -> Balance {
    match side {
        // Selling YES: mint at $1, sell at price, keep NO
        // Net cost: $1 - price per unit
        Side::SellYes => (1.0 - price) * quantity,

        // Buying YES: pay price per unit
        Side::BuyYes => price * quantity,

        // Selling NO: mint at $1, sell at (1-price), keep YES
        // Net cost: $1 - (1-price) = price per unit
        Side::SellNo => price * quantity,

        // Buying NO: pay (1-price) per unit
        Side::BuyNo => (1.0 - price) * quantity,
    }
}
```

Note: SellYes and BuyNo have same capital cost formula. SellNo and BuyYes have same formula. This is because of the minting mechanics.
