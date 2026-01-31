# Sybil V2: Architecture & Design Rationale

## Overview

Sybil V2 is a **prediction market matching engine** built on **Frequent Batch Auctions (FBA)**. It solves the matching problem: given orders with complex payoff structures across multiple markets and limited liquidity, find the welfare-maximizing matching while respecting all constraints.

---

## Solvers Pipeline

The pipeline uses a **multi-phase architecture** where each solver handles specific constraints and passes results downstream.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         SOLVING PIPELINE                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────────┐    ┌─────────────────┐        │
│  │ LocalSolver  │───▶│ NegriskSolver    │───▶│  MmAllocator    │        │
│  │ (Phase 1)    │    │ (Phase 2)        │    │  (Phase 3)      │        │
│  └──────────────┘    └──────────────────┘    └─────────────────┘        │
│         │                    │                       │                   │
│         ▼                    ▼                       ▼                   │
│  Per-market prices    Arbitrage fills        Budget-feasible fills      │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────┐       │
│  │              Partial Solvers (Parallel)                       │       │
│  │  ┌─────────────┐  ┌─────────────┐  ┌───────────────────┐     │       │
│  │  │GreedySolver │  │ MilpSolver  │                           │       │
│  │  └─────────────┘  └─────────────┘  └───────────────────┘     │       │
│  └──────────────────────────────────────────────────────────────┘       │
│                              │                                           │
│                              ▼                                           │
│                    ┌─────────────────┐                                  │
│                    │ SolutionCombiner│ (MWIS)                           │
│                    └─────────────────┘                                  │
│                              │                                           │
│                              ▼                                           │
│                    ┌─────────────────┐                                  │
│                    │    Verifier     │                                  │
│                    └─────────────────┘                                  │
└─────────────────────────────────────────────────────────────────────────┘
```

### Phase 1: LocalSolver (Price Discovery)

**File**: `crates/matching-solver/src/local_solver.rs`

**Constraints**:
- Price normalization: For N outcomes, prices must sum to $1.00 (NANOS_PER_DOLLAR)
- Unified liquidity: Market makers mint "complete sets" at $1
- Min/max fill constraints on orders
- All-or-none (AON) constraints

**Optimization Target**:
```
maximize Σ (limit_price - clearing_price) × fill_qty
```
This is welfare maximization—the total surplus captured by traders.

**Why This Solver First**:
- Per-market clearing is **fast** (O(n log n) per market)
- Produces baseline prices needed by downstream solvers
- Single-market orders (~80% of volume) are fully handled here
- No cross-market dependencies to resolve yet

**Output**: `HashMap<MarketId, Vec<Nanos>>` (clearing prices per outcome)

---

### Phase 2: NegriskSolver (Arbitrage Exploitation)

**File**: `crates/matching-solver/src/specialized/negrisk.rs`

**Purpose**: When prices for mutually exclusive outcomes don't sum to exactly $1, there's an arbitrage opportunity. Instead of artificially adjusting prices (which destroys welfare), we create arbitrage fills that exploit the mispricing.

**Two Cases**:
- **Negrisk** (sum < $1): Buy all outcomes for less than $1, guaranteed $1 payout
- **Posrisk** (sum > $1): Sell all outcomes for more than $1, only pay $1 to winner

**Example**:
If an election has three candidates with YES prices:
- Trump: 40¢, Biden: 35¢, Other: 15¢ → Total: 90¢

An arbitrageur can buy one share of each for 90¢, with guaranteed $1 payout.
This is a 10¢ risk-free profit per share, which **adds welfare**.

**Why This Approach**:
- Previous approach (PriceProjector) adjusted prices, which could invalidate orders and **destroy welfare**
- Negrisk creates real fills that pass verification
- Models what actually happens in a market (arbitrageurs exploit inconsistencies)
- Welfare is correctly attributed to the arbitrage fills

**Output**: Arbitrage orders and fills with positive welfare contribution

---

### Phase 3: MmAllocator (Budget Allocation)

**File**: `crates/matching-solver/src/mm_allocator.rs`

**Constraints**:
- **Capital budget**: Total capital used ≤ MM's max_capital
- **Capital calculation**:
  - Buy YES @ price P: capital = P × qty
  - Sell YES @ price P: capital = (1 - P) × qty
  - (Symmetric for NO outcome)

**Optimization Target**:
```
maximize Σ welfare(order) × activate(order)
subject to capital_used(MM) ≤ max_capital(MM) for all MMs
```

**Algorithm**:
1. Compute actual capital for each order from fills (not estimates)
2. Sort orders by welfare/capital ratio (greedy heuristic)
3. Activate orders greedily until budget exhausted
4. For interacting MMs: use fixed-point iteration to converge

**Why This Solver Third**:
- MM budget depends on clearing prices (bilinear constraint)
- Must have consistent prices first
- Greedy allocation is fast and gives good approximation
- Fixed-point handles MM interactions when budgets overlap

**Output**: `activated_orders: Vec<u64>` (which MM orders to fill)

---

### Partial Solvers (Parallel Exploration)

These solvers run **in parallel** to explore alternative solutions:

#### GreedySolver
**File**: `crates/matching-solver/src/greedy.rs`

**Constraint**: All order constraints (min/max fill, AON, limit price)

**Optimization**: Greedy by welfare potential = limit_price × max_fill

**Why Include**: Fast baseline (O(n log n)), provides solution even if other solvers fail

#### MilpSolver (Optional)
**File**: `crates/matching-solver/src/milp.rs`

**Constraint**: Full ILP formulation of matching problem

**Optimization**: Provably optimal welfare (given time budget)

**Why Include**: Gold standard for comparison, catches cases heuristics miss

#### (Multi-market solver — TODO)

No scalable multi-market clearing solver is currently implemented. Bundle and
spread orders are only matched if they clear within per-market price discovery.
A naive Arrow-Debreu LP has O(2^k) solvency constraints, which is impractical
when bundle orders chain many markets into one connected component.

---

### Solution Combination (MWIS)

**Files**: `crates/matching-solver/src/combiner/`

When multiple solvers produce partial solutions, they may conflict. MWIS (Maximum Weight Independent Set) selects the best non-conflicting subset.

**Conflict Graph**:
- Nodes = fills from all partial solutions
- Edges = pairs of fills that cannot coexist (same order, liquidity conflicts)

**Optimization**:
```
maximize Σ welfare(fill) for selected fills
subject to: no two selected fills conflict
```

**Algorithms Available**:
- **Greedy**: weight/(1+degree) priority
- **RandomizedGreedy**: Multiple iterations
- **ExactILP**: Optimal (requires milp feature)

---

## Why This Order Makes Sense

The pipeline order follows **dependency resolution**:

```
1. LocalSolver    →  Need prices before anything else
2. NegriskSolver  →  Exploit price inconsistencies with welfare-adding fills
3. MmAllocator    →  Need MM allocation before final fills
4. Partial Solvers → Explore alternatives with all constraints known
5. Combiner       →  Select best non-conflicting fills
```

**Key insight**: Each phase handles constraints that depend on previous phase outputs:
- NegriskSolver needs raw prices to detect arbitrage
- MmAllocator needs stable prices for capital calculation
- Partial solvers explore alternatives with all constraints known

**Fixed-Point Iteration**: The pipeline can iterate until convergence:
```
for iter in 0..max_iterations:
  Phase 1-3
  if welfare_delta < threshold: break
```
This handles cases where MM allocation affects prices.

---

## Optimization Outcomes Summary

| Solver | Objective | Complexity | Guarantee |
|--------|-----------|------------|-----------|
| LocalSolver | max welfare | O(n log n) | Optimal per-market |
| NegriskSolver | max arbitrage welfare | O(groups × markets) | Adds all exploitable arb |
| MmAllocator | max welfare/budget | O(n log n) | Greedy approx |
| GreedySolver | max welfare | O(n log n) | Heuristic |
| MilpSolver | max welfare | Exponential | Optimal (with timeout) |
| MWIS Combiner | max welfare | NP-hard | Greedy/optimal hybrid |

---

## Simulation System

### How Orders Are Placed

**File**: `crates/matching-scenarios/src/scenario.rs`

Orders are generated to mimic real market participants:

```rust
ScenarioConfig {
    num_markets: 30,
    num_orders: 3000,
    bundle_fraction: 0.15,      // 15% multi-market orders
    spread_fraction: 0.05,      // 5% relative value trades
    aon_fraction: 0.10,         // 10% all-or-none
    liquidity_scarcity: 0.7,    // Supply/demand ratio
    hot_market_fraction: 0.15,  // High-demand markets
    num_mms: 5,                 // Market makers
    mm_budget_min: 100_000,
    mm_budget_max: 1_000_000,
}
```

**Order Types Generated**:
1. **Simple orders** (70%): Single-market limit orders via `outcome_buy()`
2. **Bundle orders** (15%): Multi-market all-or-none via `bundle_yes()`
3. **Spread orders** (5%): Two-market relative value via `spread()`
4. **MM orders** (10%): Buy/sell pairs at aggressive prices

### Why This Simulates Real Markets

**Realistic Features Modeled**:

1. **Order Mix**: Real prediction markets have ~15% complex orders (bundles/spreads), matching our simulation

2. **Market Maker Behavior**:
   - Post across multiple markets with budget constraints
   - Aggressive pricing (2-8% through fair value)
   - Capital efficiency: 10x budget capacity (flash liquidity)

3. **Liquidity Microstructure**:
   - Multi-level order books (3 levels per outcome)
   - Bids/asks positioned around fair price
   - Hot markets have tighter liquidity (higher scarcity)

4. **Price Normalization**: For mutually exclusive outcomes, prices sum to $1 (no-arbitrage)

5. **Atomic State Space**: Binary encoding for joint outcomes (up to 32 states per order)

### Why This Makes Sense

**FBA Advantages**:
- All orders matched simultaneously at uniform clearing price
- Prevents front-running (order submission order irrelevant)
- Protects passive liquidity providers
- Fair price discovery through batch mechanics

**Cross-Market Realism**:
- Traders want correlated positions (e.g., "Team A wins AND Game > 50 points")
- Cannot synthesize these from single-market positions alone
- Bundle orders create genuinely new securities

---

## Alternatives & Integration Points

### Alternative Solving Approaches

| Approach | Trade-off | When to Use |
|----------|-----------|-------------|
| **Pure MILP** | Optimal but slow | Small problems (<500 orders) |
| **Pure Greedy** | Fast but suboptimal | Real-time matching |
| **Pipeline (current)** | Balanced | Production use |
| **External Solver** | Flexible | Specialized algorithms |

### Pipeline Variants

```rust
Pipeline::current()       // LocalSolver → MmAllocator (fast)
Pipeline::iterative()     // + Fixed-point iteration
Pipeline::with_negrisk()  // + NegriskSolver for arbitrage (recommended)
Pipeline::full_platform() // + MWIS combination
```

### Integration Points

**1. Custom Price Discoverer**:
```rust
impl PriceDiscoverer for MyCustomSolver {
    fn discover_prices(&self, problem: &Problem) -> PriceDiscoveryResult;
}
```

**2. Custom Order Allocator**:
```rust
impl OrderAllocator for MyAllocator {
    fn allocate(&self, constraints: &[MmConstraint], ...) -> AllocationResult;
}
```

**3. Custom Partial Solver**:
```rust
impl PartialSolver for MySolver {
    fn solve_partial(&self, problem: &Problem) -> PartialSolution;
}
```

**4. Pipeline Builder API**:
```rust
Pipeline::builder()
    .price_discoverer(LocalSolver::new())
    .negrisk_solver(NegriskSolver::new())
    .allocator(MmAllocator::new())
    .partial_solver(GreedySolver::new())
    .partial_solver(MyCustomSolver::new())
    .use_fixed_point(true)
    .combine_with_mwis(true)
    .build()
```

### External System Integration

**Inputs**:
- Orders: `matching-engine/src/order.rs` defines the `Order` struct
- Liquidity: `matching-engine/src/book.rs` defines order books
- Markets: `matching-engine/src/market.rs` defines market structure

**Outputs**:
- Fills: `(order_id, fill_qty, fill_price, welfare)`
- Prices: `HashMap<MarketId, Vec<Nanos>>`
- Verification: `verifier.rs` provides ZK-proof compatible validation

**API Entry Point**:
```rust
let pipeline = Pipeline::with_negrisk();
let result = pipeline.solve(&problem);
// result.fills, result.prices, result.total_welfare
```

---

## Key Files Reference

| Component | File | Lines |
|-----------|------|-------|
| Pipeline orchestration | `matching-solver/src/pipeline.rs` | 1000+ |
| Per-market clearing | `matching-solver/src/local_solver.rs` | 600+ |
| Negrisk arbitrage | `matching-solver/src/specialized/negrisk.rs` | 450+ |
| Budget allocation | `matching-solver/src/mm_allocator.rs` | 400+ |
| Greedy solver | `matching-solver/src/greedy.rs` | 150+ |
| MILP solver | `matching-solver/src/milp.rs` | 400+ |
| MILP solver | `matching-solver/src/milp.rs` | 400+ |
| Solution combination | `matching-solver/src/combiner/mod.rs` | 250+ |
| MWIS algorithms | `matching-solver/src/combiner/mwis.rs` | 250+ |
| Result validation | `matching-solver/src/verifier.rs` | 400+ |
| Scenario generation | `matching-scenarios/src/scenario.rs` | 643 |
| CLI simulation | `matching-sim/src/main.rs` | 1100+ |

---

## Design Philosophy

1. **Modular Competition**: Solvers compete, MWIS selects best combination
2. **Welfare Maximization**: Objective is total user surplus, not platform extraction
3. **Fairness First**: FBA eliminates front-running, uniform clearing price protects LPs
4. **Constraint Separation**: Each solver handles specific constraints, clean interfaces
5. **Verifiability**: All results can be verified (ZK-proof compatible)
