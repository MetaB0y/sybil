# Market Maker Constraints: Engine or Market Decision?

## Current State

The engine has `MmConstraint` which enforces capital budget limits on market makers:

```rust
MmConstraint::new(MmId(1), 10_000_000_000) // $10k budget
    .with_order(order_1)
    .with_order(order_2)
```

The solver (`mm_allocator.rs`) then uses Lagrangian relaxation to decide which MM orders to fill while respecting the budget constraint.

## The Core Question

**Should MM capital constraints live in the matching engine, or should MMs simply manage their own risk by choosing what orders to post?**

---

## Argument FOR Keeping MM Constraints (Status Quo)

### 1. Flash Liquidity / Virtual Market Making

The constraint enables a powerful feature: MMs can post liquidity across MANY markets simultaneously with limited capital, and the system figures out the optimal allocation at clearing time.

**Example**: An MM with $10k capital wants to provide liquidity on 100 markets. Without constraints:
- Must choose which 10 markets get $1k each
- Remaining 90 markets have no MM liquidity

With constraints:
- Post $1k of liquidity on all 100 markets
- At clearing time, system optimally allocates the $10k budget to wherever it's most needed
- Markets that don't clear don't consume capital
- Result: Better liquidity across all markets

### 2. Clearing Price Dependency

MM capital usage depends on clearing prices, which aren't known until matching completes:
- Selling YES at 60¢ costs 40¢ per share (you owe $1 if YES happens)
- Selling YES at 40¢ costs 60¢ per share

Without engine support, MMs must guess prices and over-collateralize, leading to inefficient capital use.

### 3. Atomic Budget Enforcement

The engine can atomically ensure capital constraints are respected. Without it:
- MM might get filled on Market A at 50¢
- Before MM can cancel, also filled on Market B at 50¢
- Now over budget, possibly insolvent

### 4. Welfare Optimization

The Lagrangian approach finds welfare-maximizing allocation of scarce MM capital. A greedy approach (first-come-first-served) would be suboptimal.

---

## Argument AGAINST (Let The Market Decide)

### 1. MMs Already Express Preferences Through Prices

If an MM doesn't want to provide liquidity at a certain price... they just don't post that quote. The limit price IS the constraint:
- Don't want to sell YES below 55¢? Post ask at 55¢
- Don't want to buy YES above 45¢? Post bid at 45¢

Capital constraints are redundant if MMs set prices correctly.

### 2. Complexity Violates Engine Minimalism

We just removed market constraints from the engine because "engine should be engine." MM constraints are arguably the same pattern:
- Engine concern: Match orders against liquidity
- NOT engine concern: Business logic about who can afford what

### 3. Capital Management is MM's Job

Professional MMs have sophisticated risk systems. They:
- Monitor positions across all markets in real-time
- Hedge dynamically
- Adjust quotes based on inventory

Forcing them through a constraint system may be overly paternalistic.

### 4. Simple Alternative Exists

Instead of constraints, MMs could:
1. Post smaller quantities they CAN afford to lose
2. Update quotes more frequently
3. Use cancel-if-filled-elsewhere logic (which could be its own order type)

### 5. Auction/Batch vs Continuous

The constraint model makes most sense for batch auctions where everything clears at once. In continuous trading (which Sybil may evolve toward), the constraint model is harder to apply.

---

## Synthesis: When Do Constraints Add Value?

| Scenario | Constraint Value | Alternative |
|----------|-----------------|-------------|
| Single-price batch auction | **High** - optimal allocation at clearing | None good |
| Frequent batch auctions (every minute) | Medium - less time to over-allocate | Post conservatively |
| Continuous trading | Low - must handle partial fills anyway | Standard MM inventory management |
| Few markets (<10) | Low - MM can manage manually | Just post what you can afford |
| Many markets (100+) | **High** - flash liquidity is powerful | Inefficient capital allocation |

---

## Initial Consideration: Move to Solver?

One might argue MM constraints should move to solver (like market constraints did). The idea:
- Solver pre-filters which MM orders to submit
- Engine just does matching

**But this doesn't work** because:
1. You can't pre-filter without knowing clearing prices
2. Clearing prices aren't known until matching completes
3. This creates a circular dependency

The solver CAN'T decide "which orders to submit" because capital usage depends on prices that only emerge FROM the matching.

---

## Final Decision: Keep in Engine

After further analysis, MM constraints ARE an engine concern because they constrain **what constitutes a valid match**.

### The Key Distinction

**Market constraints** (removed): "If A wins, B must win"
- Metadata about market relationships
- Solver can handle via price adjustments
- Not about matching validity

**MM constraints** (keep): "Orders X, Y, Z cannot ALL be filled if combined capital > budget"
- Directly constrains valid matching solutions
- Analogous to liquidity constraints
- Requires atomicity during matching

### Analogy

| Constraint Type | Example | Engine Concern? |
|----------------|---------|-----------------|
| Liquidity | "Can't fill 1000 if only 500 available" | ✅ Yes |
| MM Budget | "Can't fill A+B+C if capital > $10k" | ✅ Yes |
| Market Relations | "If Trump wins, Biden loses" | ❌ No (solver) |

### Why Engine, Not Solver

1. **Atomicity**: Matching and constraint enforcement must be simultaneous
2. **Price-dependent**: Capital usage depends on clearing prices, unknown until match
3. **Validity**: An MM-violating solution is invalid, not just suboptimal

### Conclusion

MM constraints stay in `matching-engine`. They're not "business logic" - they're constraints on valid solutions, like liquidity itself.

The engine is minimal but not simplistic. It handles:
- Orders
- Liquidity constraints
- MM budget constraints
- Matching

All of these constrain what matches are possible.
