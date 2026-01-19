# Sybil V2: Market Structure Synthesis

**A comprehensive explanation of why this market structure is good, fair, and elegant.**

---

## Executive Summary

Sybil V2 is a prediction market exchange built on Frequent Batch Auctions (FBA) with interlocking innovations:

1. **Welfare-maximizing matching** through patch-based solving and MWIS combination
2. **Uniform Clearing Price (UCP)** that makes the market fundamentally fair
3. **MM budget constraints** handled via Lagrangian relaxation

The design philosophy is simple: **markets should maximize total user welfare, not extraction**.

---

## Part 1: The Core Market Architecture

### 1.1 Frequent Batch Auctions: The Foundation of Fairness

Instead of continuous order book matching (CLOB), Sybil collects orders over a time window and matches them simultaneously at a single clearing price.

**Why FBA is Fair:**

1. **No Front-Running** — Order submission order doesn't matter. A trader can't jump ahead by paying higher fees or submitting faster.

2. **Fair Price Discovery** — The clearing price emerges from the balance of supply and demand, not from sequential price moves.

3. **Passive Liquidity Providers Get Protected** — Consider:
   - You quote: "Sell 100 at $0.50" (your stale quote)
   - Informed traders arrive: "Buy 10,000 at $0.95-$0.99"
   - **In CLOB**: You get picked off at $0.50 before you can update
   - **In FBA**: Clearing price moves to ~$0.95. You sell at $0.95, not $0.50!

The Uniform Clearing Price pulls your fill price up to the fair level based on actual demand.

### 1.2 Linear Constraint Orders: Unified Expression

Instead of separate order types (simple limit, spread, bundle, butterfly, conditional), Sybil uses a single representation: **payoff vectors over atomic world states**.

```
Order = {
  markets: which markets involved,
  payoffs: vector specifying what you win/lose in each possible outcome,
  limit_price: max you'll pay per unit,
  min/max_fill: quantity constraints
}
```

**Examples:**
- **Simple limit order**: Payoff [+1, 0] → buy YES
- **Spread** ("Long A, Short B"): Payoff [0, -1, +1, 0]
- **Bundle** ("Buy A AND B"): Payoff [1, 0, 0, 0] → win only if both happen
- **Conditional**: Payoff vector + price condition → activate when threshold crossed

Single representation = **single solver handles all order types**. No special cases.

### 1.3 Multi-Market Cross-Market Matching

**The Problem:**

If you want to bet "Trump wins AND Republicans win Senate", in separate markets you might:
- Get only one leg filled (bad hedging)
- Pay inflated prices from correlation blindness
- Miss the opportunity entirely

**Sybil's Solution: Patch-Based Solving**

```
Phase 1: Base solution
  - Solve each market independently using FBA
  - O(n log n), parallelizable

Phase 2: Cross-market patches
  - Specialized solvers propose "patches" that fill cross-market orders
  - Each patch: affected markets, fills, price adjustments, welfare delta

Phase 3: Solution combination
  - Build conflict graph: patches touching same market conflict
  - Solve Maximum Weight Independent Set (MWIS)
  - Select best non-conflicting patches
```

**Why This Design Wins:**

- **Scalable**: Base solution is cheap; patches only for orders that need them
- **Modular**: New specialized solvers can propose patches independently
- **Combinable**: MWIS lets multiple solvers contribute non-conflicting improvements
- **Welfare-maximizing**: Each patch only counts if it improves welfare

### 1.4 Welfare Maximization: The Objective Function

**Definition:**
```
Welfare = Σ (limit_price - clearing_price) × fill_qty
```

This captures exactly what we want:
- **Buyer welfare**: Value received minus price paid
- **Seller welfare**: Price received minus cost
- **Total welfare**: Sum of all user surplus

**Why This is Fair:**

- Orders that create the most value get filled first
- Everyone's surplus is respected equally
- No arbitrary prioritization (like "first come, first served" which rewards speed)

---

## Part 2: Fairness Through Mechanism Design

### 2.1 Arbitrage Prevention: Keeping Prices Consistent

**The Problem:**

If you can independently buy "Trump wins" at $0.60 and sell "Republican wins" at $0.50, and Trump winning implies Republican winning, someone has riskless profit and the system is mispriced.

**Sybil's Solution: Specialized Solvers**

Three specialized solvers find and fix these:

1. **ArbitrageDetector**: Finds constraint-based arbitrage
   - Detects when A→B but price(A) > price(B)
   - Creates patches that exploit the mispricing
   - Profits fund welfare improvements for users

2. **BundleDecomposer**: Finds underpriced complement sets
   - Example: 4 bundles covering all outcomes at $1.03 total
   - Guaranteed $1.00 payout = $0.03 profit
   - Fills all atomically to lock in the arbitrage

3. **ChainFinder**: Exploits implication chains
   - Follows A→B→C→D chains
   - If you can buy the root cheaply, you get all exposures for less

**Why This is Fair:**

- Arbitrage profits improve everyone's matching quality, not a single MM
- Prices converge to consistency across markets
- Users benefit from tighter pricing through better patches

### 2.2 Constraint Systems and Market Relationships

**Types of Constraints:**

```
Implication: A wins → B wins (hierarchical)
SumToOne: Outcomes within market mutually exclusive
Hierarchy: Tournament bracket relationships
MutuallyExclusive: At most one can happen
ExactlyOne: Exactly one must happen
```

**Example: Tournament Bracket**

```
Semifinals: A vs B, C vs D
Finals: Winner1 vs Winner2
Champion: Finals winner

Constraints:
  - Exactly one of {A, B} makes finals
  - Exactly one of {C, D} makes finals
  - Exactly one of {A, B, C, D} wins championship
  - To win championship, must win semifinals AND finals

Arbitrage protection:
  - Can't have different prices for "A wins championship"
    and "A wins semifinals AND wins finals"
```

---

## Part 3: The Solver Platform

### 3.1 The Complete Pipeline

```
Problem Input
    ↓
Phase 1: Base Solution
    - Per-market clearing with price normalization
    - Fast, O(n log n)
    ↓
Phase 2: Parallel Solvers
    - LocalSolver (per-market clearing)
    - MmAllocator (MM budget allocation)
    - Specialized: arbitrage, bundles, chains
    ↓
Phase 3: Solution Combination
    - Flatten all solutions into patches
    - Build conflict graph
    - Solve MWIS with welfare as weight
    ↓
Final Solution
    - All fills validated
    - Prices checked for consistency
    - Welfare computed
```

### 3.2 Why Multiple Solvers + MWIS

**Individual Solver Strengths:**

- **LocalSolver**: Fast per-market clearing with normalization
- **MmAllocator**: MM budget allocation via Lagrangian relaxation
- **Arbitrage**: Finds constraint-based mispricings
- **BundleDecomposer**: Finds complementary bundles
- **ChainFinder**: Exploits implication chains

**MWIS Combination Benefit:**

Instead of picking the best solver:
- Combine their non-conflicting improvements
- Each solver contributes fills others missed
- Total welfare = sum of non-conflicting patches

```
Example:
  LocalSolver finds: Orders A, B, C → welfare $100
  MmAllocator finds: Orders B, D, E → welfare $105
  Specialized: Order F (arbitrage) → welfare $10

  Conflict: A conflicts with D (same market)

  MWIS picks: B (all), E (MmAllocator), C (LocalSolver), F (Specialized)
  Total welfare: $125 > MmAllocator's $105 alone
```

---

## Part 4: Why This Design is Good, Fair, and Elegant

### 4.1 Goodness: Welfare Maximization

**Property**: Every filled order increases total welfare.

**Mechanism**:
- Welfare = value to user - price paid
- Only fill orders where limit_price > clearing_price
- Maximize sum of (limit_price - clearing_price) × qty

**Fairness**: Users aren't ranked by speed or luck. Orders fill based on value creation.

### 4.2 Fairness: Price Protection and No Front-Running

**Property**: Execution price doesn't depend on submission order.

**Mechanisms**:
- **UCP**: Everyone in market gets same clearing price
- **Batch**: All orders matched simultaneously
- **No sequencing**: Order of execution doesn't affect price

### 4.3 Fairness: Passive LP Protection

**Property**: Passive liquidity providers get fair prices, not predatory extraction.

**Mechanisms**:
- **UCP pulls prices**: Supply/demand imbalance moves clearing price to fair level
- **Batch matching**: No one can front-run stale quotes

### 4.4 Elegance: Unified Order Representation

**Property**: All order types use payoff vectors. Single solver architecture.

**Benefit**:
- No special cases in code
- New order types don't need solver modifications
- Constraints naturally expressed
- Arbitrage detection generic

### 4.5 Elegance: Patch-Based Solving

**Property**: Decompose hard problem into base solution + local improvements.

**Benefit**:
- Base solution O(n log n), trivially parallelizable
- Patches are small, specialized solvers handle them
- MWIS combination is elegant, reusable
- Non-conflicting patches combine naturally

### 4.6 Elegance: Constraint-Based Arbitrage

**Property**: Arbitrage = price inconsistency. Fixing arbitrage = fixing prices.

**Benefit**:
- No separate constraint enforcement logic
- Arbitrage detection IS constraint validation
- Specialized solvers naturally find mispricings
- Fair pricing emerges from welfare maximization

---

## Part 5: Design Decisions and Rationale

### 5.1 Why FBA Over CLOB

| Aspect | CLOB | FBA |
|--------|------|-----|
| Front-running | Yes | No |
| Price discovery | Sequential | Equilibrium |
| Passive LP extraction | Severe | Mild (UCP protects) |
| Matching efficiency | Immediate but suboptimal | Delayed but optimal |

**Decision**: FBA wins on fairness.

### 5.2 Why Payoff Vectors Over Order Types

| Approach | Pros | Cons |
|----------|------|------|
| Type system (enum) | Type-safe, explicit | Combinatorial explosion |
| Payoff vectors | Unified, general | Less explicit |

**Decision**: Payoff vectors. Generality wins. Add type helpers on top.

### 5.3 Why MWIS For Combination

| Approach | Mechanism | Pros | Cons |
|----------|-----------|------|------|
| Winner-takes-all | Best single | Simple | Discards good partials |
| MWIS | Combine non-conflicting | Optimal | NP-hard |

**Decision**: MWIS. Graph is sparse in practice. Combinatorial benefit too large.

### 5.4 Why Welfare Over Volume

| Objective | What Fills | Fairness |
|-----------|-----------|----------|
| Volume | Most units | Random priority |
| Welfare | Most valuable | Fair to all |
| Revenue | Highest-fee | Favors large traders |

**Decision**: Welfare. Only fairness-respecting objective.

---

## Part 6: Comparisons to Related Systems

### 6.1 vs Traditional CLOB (Binance, Nasdaq)

| Property | CLOB | Sybil |
|----------|------|-------|
| Front-running | Yes | No |
| Fair pricing | No | Yes |
| Welfare optimization | No | Yes |
| Complex orders | Limited | Full |
| Cross-market | No | Yes |

### 6.2 vs MEV-Boost (Ethereum)

| Property | MEV-Boost | Sybil |
|----------|-----------|-------|
| Role | Block builder auction | Solver orchestration |
| Selection | Winner-takes-all | MWIS combination |
| Objective | Builder profit | User welfare |

**Similarity**: Both use patch-based solving with combination.
**Difference**: Sybil maximizes welfare, not extraction.

### 6.3 vs CoW Protocol (Ethereum Batch Auctions)

| Property | CoW | Sybil |
|----------|-----|-------|
| Order type | Token swaps | Prediction markets |
| Solver model | External, MEV | Internal, welfare |
| Cross-market | Limited | Full |
| Fairness | Intent-based | Welfare-based |

**Similarity**: Both batch orders for fair execution.
**Difference**: Sybil is domain-specific, welfare-focused.

---

## Part 7: Key Guarantees

### Fairness Guarantees

1. **No front-running**: Submission order doesn't affect execution price
2. **UCP protection**: All fills in a market get same clearing price
3. **Welfare-respecting**: Orders fill by value creation, not luck
4. **Atomic bundles**: Multi-market bundles fill completely or not at all
5. **Constraint respect**: Prices consistent with market relationships

### Efficiency Guarantees

1. **Welfare-optimal**: Matches all orders that improve welfare
2. **MM budget constraints**: Respected via Lagrangian relaxation
3. **Computation-efficient**: Base O(n log n), manageable overhead
4. **Scalable**: Proven to 50K+ orders

### Transparency Guarantees

1. **Deterministic**: Identical inputs → identical outputs
2. **Auditable welfare**: Can verify total welfare improvement

---

## Conclusion: Why This Matters

Current market structures aren't fair:
- CLOBs favor fast traders
- Single-batch auctions have single winners
- Call auctions require manual operation

Sybil's innovation combines:
1. **Batch auctions** for fairness (no front-running)
2. **Welfare maximization** for efficiency (right orders fill)
3. **Cross-market matching** for completeness (complex bets possible)
4. **MM budget constraints** for capital efficiency (MMs can quote widely)
5. **Constraint arbitrage** for price consistency (markets correct)
6. **MWIS combination** for elegance (solvers cooperate)

The result is a market structure that is:
- **Fair**: Everyone gets equivalent treatment
- **Efficient**: Orders fill based on value
- **Elegant**: Unified design, single solving approach
- **Scalable**: Handles complex orders at scale
- **Incentive-aligned**: MMs, solvers, users all benefit

**This is why Sybil V2 is good**: it maximizes fairness AND welfare. It's not zero-sum. Improving one group's outcome improves everyone's.
