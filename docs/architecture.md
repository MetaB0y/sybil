# Sybil V2: Architecture

## Overview

Sybil V2 is a prediction market exchange using Frequent Batch Auctions (FBA) with:
- **LP-based order representation** - Orders are linear constraints, not simple limit orders
- **Solver network** - Multiple solvers propose solutions, best combination selected
- **Cross-market support** - Orders can span multiple correlated markets
- **Uniform clearing price** - All fills in a market execute at the same price

## Key Design Decisions

### 1. Linear Constraint Orders

Orders are expressed as linear constraints over outcomes:
```rust
struct Order {
    markets: [MarketId; MAX_MARKETS],
    num_markets: u8,
    payoffs: [i8; MAX_STATES],    // Payoff per state
    limit_price: Nanos,           // Max willing to pay per unit
    min_fill: Qty,
    max_fill: Qty,
}
```

This representation supports:
- Simple limit orders (single market, binary payoff)
- Spread trades (long A, short B)
- Bundle orders (A AND B must both win)
- Conditional orders (activated by price thresholds)

### 2. Patch-Based Solving

The matching problem is solved in two phases:

**Phase 1: Base Solution**
- Solve each market independently using FBA
- O(n log n) per market, trivially parallelizable

**Phase 2: Cross-Market Patches**
- Specialized solvers propose "patches" that fill cross-market orders
- Patches specify: affected markets, fills, price adjustments, welfare delta
- Non-conflicting patches selected via MWIS (Maximum Weight Independent Set)

### 3. Solution Combination via MWIS

When multiple solvers propose solutions, they're combined using MWIS:
- Build conflict graph: nodes = patches, edges = market overlaps
- Select maximum-weight independent set
- Greedy or randomized greedy algorithms work well in practice

### 4. Uniform Clearing Price (UCP)

All fills in a market execute at the same clearing price:
- No front-running possible (batch ordering doesn't matter)
- Price determined by supply/demand equilibrium
- Welfare = sum of (value - price) for buyers + (price - cost) for sellers

### 5. Integer Arithmetic

All amounts use fixed-point integer arithmetic:
```rust
type Nanos = i64;  // Price in nanos (10^-9), e.g., 500_000_000 = $0.50
type Qty = i64;    // Quantity in base units
```

---

## Complexity Analysis

### Sources of Complexity (Ranked)

#### Critical: Cross-Market Coupling

Several features create dependencies between markets:
- **Budget constraints** - User's balance spans multiple markets
- **Bundle orders** - "Buy A AND B" must fill both or neither
- **Spread trades** - "Long A, Short B" links two markets

**Impact**: Independent markets can be solved in parallel; coupled markets must be solved together.

**Mitigation**:
- Limit max markets per order (currently MAX_MARKETS_PER_ORDER = 4)
- Most orders are single-market in practice
- Cross-market orders create local coupling, not global

#### Significant: MWIS is NP-Hard

Selecting optimal non-conflicting patches is NP-hard in general.

**Mitigations**:
1. Conflict graph is sparse (patches affect few markets)
2. Greedy gives good approximations
3. Randomized parallel greedy explores many orderings
4. Time budget enforces early termination

#### Manageable: LP Solving

For each market, solving the FBA is a small LP:
- Variables: fill fractions, clearing price
- Constraints: supply = demand, price limits
- Well-understood, efficient solvers exist (HiGHS)

### Scaling Limits

| Resource | Practical Limit | Determined By |
|----------|-----------------|---------------|
| Orders per batch | ~10K | LP solver capacity |
| Markets per batch | ~1K | State management |
| Cross-market orders | ~100 | LP coupling complexity |
| Solvers per batch | ~10 | Combination time |

---

## Solver Architecture

### Solver Types

#### 1. Greedy Solver
- Processes orders by welfare contribution
- Fills orders when liquidity available
- O(n log n) per market
- Good baseline, fast execution

#### 2. MILP Solver
- Formulates full problem as Mixed Integer LP
- Uses HiGHS for optimization
- Near-optimal for single-market
- Slower but higher quality

#### 3. Randomized Greedy Solver
- Runs multiple random orderings
- Takes best result across iterations
- Balances speed vs quality
- Good for exploration

#### 4. Solver Platform
- Orchestrates multiple specialized solvers
- Combines solutions via MWIS
- Produces contribution statistics
- Production solver choice

### Specialized Solvers (within Platform)

| Solver | Purpose | Strategy |
|--------|---------|----------|
| Greedy | Baseline | Sort by welfare, fill greedily |
| MILP | Optimization | Full LP formulation |
| Arbitrage | Price consistency | Find cross-market mispricings |
| BundleDecomposer | Multi-market orders | Decompose bundles into fills |
| ChainFinder | Implication chains | Follow market correlations |

### Solver Economics

Revenue sources:
- Fee share from matched volume
- JIT liquidity profits (future)
- Arbitrage capture

The platform currently combines solver outputs without external incentives.

---

## Data Flow

```
Orders submitted
       │
       ▼
┌──────────────┐
│   Problem    │  (orders + liquidity + constraints)
└──────┬───────┘
       │
       ▼
┌──────────────────────────────────────────────────┐
│                Solver Platform                    │
│  ┌────────┐ ┌──────┐ ┌─────────┐ ┌───────────┐  │
│  │ Greedy │ │ MILP │ │ Arb     │ │ Bundle    │  │
│  └───┬────┘ └──┬───┘ └────┬────┘ └─────┬─────┘  │
│      │         │          │            │         │
│      └────────┬┴──────────┴────────────┘         │
│               │                                   │
│               ▼                                   │
│        ┌────────────┐                            │
│        │ Combiner   │  (MWIS on conflict graph)  │
│        └─────┬──────┘                            │
└──────────────┼───────────────────────────────────┘
               │
               ▼
┌──────────────────────┐
│   MatchingResult     │  (fills + prices + welfare)
└──────────────────────┘
```

---

## Module Structure

```
crates/
├── matching-engine/     # Core types
│   ├── src/
│   │   ├── lib.rs
│   │   ├── order.rs         # Order representation
│   │   ├── fill.rs          # Fill execution
│   │   ├── liquidity.rs     # Order book / pool
│   │   ├── problem.rs       # Problem definition
│   │   ├── market.rs        # Market definitions
│   │   └── state.rs         # State space
│
├── matching-solver/     # Solving algorithms
│   ├── src/
│   │   ├── lib.rs
│   │   ├── greedy.rs        # Greedy solver
│   │   ├── milp.rs          # MILP solver
│   │   ├── randomized.rs    # Randomized greedy
│   │   ├── platform.rs      # Multi-solver platform
│   │   ├── combiner/        # Solution combination
│   │   │   ├── mod.rs
│   │   │   ├── conflict.rs  # Conflict graph
│   │   │   └── mwis.rs      # MWIS algorithms
│   │   └── specialized/     # Specialized solvers
│
├── matching-scenarios/  # Test scenarios
│   ├── src/
│   │   ├── stress.rs        # Stress testing
│   │   └── random.rs        # Random generation
│
└── matching-sim/        # CLI tool
    └── src/main.rs
```

---

## Future Considerations

### External Solvers

Current design runs solvers in-process. Future design:
- External solvers submit solutions via API
- TEE validates solutions
- Solver staking/slashing for misbehavior

### ZK Proofs

State transitions could be proven with ZK:
- Batch execution proofs
- Settlement finality
- Trustless verification

---

## References

- [Frequent Batch Auctions (Budish et al.)](https://faculty.chicagobooth.edu/eric.budish/research/HFT-FrequentBatchAuctions.pdf)
- [Maximum Weight Independent Set](https://en.wikipedia.org/wiki/Independent_set_(graph_theory))
- [HiGHS LP Solver](https://highs.dev/)
