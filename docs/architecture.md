# Sybil V2: Architecture & Design Rationale

## Overview

Sybil V2 is a **prediction market matching engine** built on **Frequent Batch Auctions (FBA)**. It solves the matching problem: given orders with complex payoff structures across multiple markets and limited liquidity, find the welfare-maximizing matching while respecting all constraints.

---

## Solvers Pipeline

The pipeline uses a **multi-phase architecture** with three solving modes. The mode is selected based on pipeline configuration:

- **Single-pass**: Runs each phase once sequentially
- **Sequential (fixed-point)**: Iterates phases until welfare converges
- **Dual decomposition**: Uses Lagrangian relaxation with subgradient updates

### Sequential Pipeline

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      SEQUENTIAL SOLVING PIPELINE                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────────┐    ┌──────────────────┐       │
│  │ LocalSolver  │───▶│ MultiMarketSolver│───▶│ NegriskSolver    │       │
│  │ (Phase 1)    │    │ (Phase 2)        │    │ (Phase 3)        │       │
│  └──────────────┘    └──────────────────┘    └──────────────────┘       │
│         │                    │                       │                   │
│         ▼                    ▼                       ▼                   │
│  Per-market prices    Bundle fills +          Arb orders (price         │
│                       adjusted prices         pressure for next iter)   │
│                                                                          │
│  ┌──────────────────┐                                                   │
│  │  MmAllocator     │                                                   │
│  │  (Phase 4)       │                                                   │
│  └──────────────────┘                                                   │
│         │                                                                │
│         ▼                                                                │
│  Budget-feasible fills                                                  │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────┐       │
│  │              Partial Solvers (Parallel)                       │       │
│  │  ┌─────────────┐  ┌───────────────────────────┐              │       │
│  │  │ MilpSolver  │  │ MultiMarketSolver (MWIS)  │              │       │
│  │  └─────────────┘  └───────────────────────────┘              │       │
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

### Dual Decomposition Pipeline

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    DUAL DECOMPOSITION PIPELINE                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────────────────────────────────┐       │
│  │ LocalSolver  │───▶│           DualMaster                     │       │
│  │ (base prices)│    │                                          │       │
│  └──────────────┘    │  for iter in 0..max_iterations:          │       │
│                      │    1. Shade orders using λ               │       │
│                      │    2. Solve per-market (LocalSolver)     │       │
│                      │    3. Validate vs original limits        │       │
│                      │    4. Greedy MM knapsack                 │       │
│                      │    5. Accumulate fills                   │       │
│                      │    6. Update λ (subgradient)             │       │
│                      │    7. Check convergence                  │       │
│                      └──────────────────────────────────────────┘       │
│                              │                                           │
│                              ▼                                           │
│  ┌──────────────────┐    ┌────────────────────────┐                    │
│  │ MultiMarketSolver│───▶│ Partial Solvers (MILP) │                    │
│  │ (bundle repricing)│   └────────────────────────┘                    │
│  └──────────────────┘                                                   │
│                              │                                           │
│                              ▼                                           │
│                    ┌─────────────────┐                                  │
│                    │    Verifier     │                                  │
│                    └─────────────────┘                                  │
└─────────────────────────────────────────────────────────────────────────┘
```

---

### Phase 1: LocalSolver (Price Discovery)

**File**: `crates/matching-solver/src/local_solver.rs`

**Constraints**:
- Price normalization: For N outcomes, prices must sum to $1.00 (NANOS_PER_DOLLAR)
- Unified liquidity: Market makers mint "complete sets" at $1
- Max fill constraints on orders

**Optimization Target**:
```
maximize Sigma (limit_price - clearing_price) * fill_qty
```
This is welfare maximization—the total surplus captured by traders.

**Why This Solver First**:
- Per-market clearing is **fast** (O(n log n) per market)
- Produces baseline prices needed by downstream solvers
- Single-market orders (~80% of volume) are fully handled here
- No cross-market dependencies to resolve yet

**Output**: `HashMap<MarketId, Vec<Nanos>>` (clearing prices per outcome)

---

### Phase 2: MultiMarketSolver (Bundle & Spread Matching)

**File**: `crates/matching-solver/src/specialized/multi_market.rs`

**Purpose**: Matches multi-market orders (bundles, spreads, conditionals) that can't be fully handled by per-market price discovery alone.

**Two Strategies**:

1. **Complement Matching**: Orders with identical markets and negated payoffs cancel perfectly (e.g., bundle_yes + bundle_sell). Standard bid >= ask matching applied to paired orders.

2. **Direct Price-Shifting (Repricing)**: Decomposes bundle orders into per-market "legs" and injects that demand into per-market supply/demand curves. Uses `PrecomputedMarket` for O(S) fast trial crossings (where S = number of supply/demand steps). Only commits to a full re-solve when the fast trial shows positive net welfare. Maintains uniform clearing price (UCP) invariant.

**Why This Phase**:
- Bundle orders create genuine cross-market coupling
- Per-market clearing alone can't fill bundles that are only profitable at adjusted prices
- Repricing allows bundle demand to shift per-market prices, unlocking new fills
- Complement matching handles the common case (paired orders) cheaply

**Output**: Bundle fills + adjusted clearing prices for affected markets

---

### Phase 3: NegriskSolver (Arbitrage Exploitation)

**File**: `crates/matching-solver/src/specialized/negrisk.rs`

**Purpose**: When prices for mutually exclusive outcomes don't sum to exactly $1, there's an arbitrage opportunity. Instead of artificially adjusting prices (which destroys welfare), we create synthetic arbitrage orders that influence prices through market forces.

**Two Cases**:
- **Negrisk** (sum < $1): Buy all outcomes for less than $1, guaranteed $1 payout
- **Posrisk** (sum > $1): Sell all outcomes for more than $1, only pay $1 to winner

**Key insight**: In the sequential pipeline, arb orders are **not** added to the final output. They serve as synthetic price-pressure orders that participate in the next iteration's LocalSolver clearing to push prices toward sum = $1. This is more principled than directly adjusting prices because the arb orders compete in the market like any other order.

**Why This Approach**:
- Previous approach (PriceProjector) adjusted prices, which could invalidate orders and **destroy welfare**
- Negrisk creates real market forces that pass verification
- Models what actually happens in a market (arbitrageurs exploit inconsistencies)
- Welfare is correctly attributed

**Output**: Synthetic arb orders (price pressure for next iteration)

---

### Phase 4: MmAllocator (Budget Allocation)

**File**: `crates/matching-solver/src/mm_allocator.rs`

**Constraints**:
- **Capital budget**: Total capital used <= MM's max_capital
- **Capital calculation**:
  - Buy YES @ price P: capital = P * qty
  - Sell YES @ price P: capital = (1 - P) * qty
  - (Symmetric for NO outcome)

**Optimization Target**:
```
maximize Sigma welfare(order) * activate(order)
subject to capital_used(MM) <= max_capital(MM) for all MMs
```

**Algorithm**:
1. Compute actual capital for each order from fills (not estimates)
2. Sort orders by welfare/capital ratio (greedy knapsack)
3. Activate orders greedily until budget exhausted
4. For interacting MMs: use fixed-point iteration to converge

**Why This Solver After Pricing**:
- MM budget depends on clearing prices (bilinear constraint)
- Must have consistent prices first
- Greedy allocation is fast and gives good approximation
- Fixed-point handles MM interactions when budgets overlap

**Output**: `activated_orders: Vec<u64>` (which MM orders to fill)

---

### Dual Decomposition (DualMaster)

**File**: `crates/matching-solver/src/dual_master.rs`

**Purpose**: Handles two types of coupling constraints in a principled way using Lagrangian relaxation:

1. **Price consistency** (lambda): YES prices across a MarketGroup must sum to $1
2. **MM budgets**: Capital usage must not exceed per-MM budget

**Algorithm**:

1. Initialize lambda = 0 for each MarketGroup
2. For each iteration (up to 10 by default):
   a. **Shade orders** using lambda — adjust limit prices to reflect price consistency pressure (YES buyers bid less when prices sum too high, and vice versa)
   b. **Solve per-market subproblems** with shaded orders + remaining liquidity via LocalSolver
   c. **Collect candidate fills**, validate against *original* (unshaded) limit prices
   d. **Separate non-MM fills** (accepted directly) from **MM fills** (sent to greedy knapsack)
   e. **Greedy MM knapsack**: Sort MM candidates by welfare/capital ratio, greedily activate within remaining budget
   f. **Accumulate fills** across iterations (fills are permanent once accepted)
   g. **Compute price residuals** (sum_yes - $1 per group)
   h. **Update lambda** via subgradient step: `lambda += step_size * residual`
   i. **Check convergence**: price residuals < 2% AND dual stability AND welfare improvement < 1%

Step size decays as `alpha_0 / sqrt(t)` (InvSqrt) or `alpha_0 / t` (InvLinear).

**Configuration** (`DualConfig`):
- `max_iterations`: 10 (default)
- `initial_step_size`: 0.5
- `primal_tolerance`: 0.02 (2% price residual)
- `dual_tolerance`: 0.001
- `welfare_tolerance`: 0.01 (1% marginal improvement)

**Key difference from sequential pipeline**: Dual decomposition handles price consistency (sum = $1) and MM budgets as Lagrangian dual constraints, whereas the sequential pipeline uses NegriskSolver for price consistency and MmAllocator separately. The dual approach is more principled; the sequential approach can be faster in practice.

---

### Partial Solvers (Parallel Exploration)

These solvers run **in parallel** to explore alternative solutions:

#### MilpSolver (Optional)
**File**: `crates/matching-solver/src/milp.rs`

**Constraint**: Full ILP formulation of matching problem

**Optimization**: Provably optimal welfare (given time budget)

**Why Include**: Gold standard for comparison, catches cases heuristics miss. Feature-gated behind `milp` (uses HiGHS via `good_lp`).

---

### Solution Combination (MWIS)

**Files**: `crates/matching-solver/src/combiner/`

When multiple solvers produce partial solutions, they may conflict. MWIS (Maximum Weight Independent Set) selects the best non-conflicting subset.

**Conflict Graph**:
- Nodes = fills from all partial solutions
- Edges = pairs of fills that cannot coexist (same order, liquidity conflicts)

**Optimization**:
```
maximize Sigma welfare(fill) for selected fills
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
1. LocalSolver         -> Need prices before anything else
2. MultiMarketSolver   -> Bundles need per-market prices to decompose legs
3. NegriskSolver       -> Exploit price inconsistencies with welfare-adding fills
4. MmAllocator         -> Need stable prices for capital calculation
5. Partial Solvers     -> Explore alternatives with all constraints known
6. Combiner            -> Select best non-conflicting fills
```

**Key insight**: Each phase handles constraints that depend on previous phase outputs:
- MultiMarketSolver needs per-market prices to evaluate bundle legs
- NegriskSolver needs cross-market prices to detect arbitrage
- MmAllocator needs stable prices for capital calculation
- Partial solvers explore alternatives with all constraints known

**Fixed-Point Iteration**: The sequential pipeline can iterate until convergence:
```
for iter in 0..max_iterations:
  Phase 1-4
  if welfare_delta < threshold: break
```
This handles cases where MM allocation or arb order injection affects prices.

**Dual Decomposition**: Alternatively, the DualMaster handles price consistency and MM budgets jointly via Lagrangian relaxation. MultiMarketSolver repricing and partial solvers run after the dual solver converges.

---

## Optimization Outcomes Summary

| Solver | Objective | Complexity | Guarantee |
|--------|-----------|------------|-----------|
| LocalSolver | max welfare | O(n log n) | Optimal per-market |
| MultiMarketSolver | max bundle welfare | O(n log n) per market | Heuristic (repricing) |
| NegriskSolver | max arbitrage welfare | O(groups * markets) | Adds all exploitable arb |
| MmAllocator | max welfare/budget | O(n log n) | Greedy approx |
| DualMaster | max welfare (joint) | O(iterations * n log n) | Convergent |
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
    liquidity_scarcity: 0.7,    // Supply/demand ratio
    hot_market_fraction: 0.15,  // High-demand markets
    num_mms: 5,                 // Market makers
    mm_budget_min: 100_000,
    mm_budget_max: 1_000_000,
}
```

**Order Types Generated**:
1. **Simple orders** (70%): Single-market limit orders via `outcome_buy()`
2. **Bundle orders** (15%): Multi-market orders via `bundle_yes()`
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
| **Sequential pipeline** | Balanced, heuristic | Default production use |
| **Dual decomposition** | Principled, handles coupling | Many market groups + MMs |
| **External Solver** | Flexible | Specialized algorithms |

### Pipeline Variants

```rust
Pipeline::current()                     // LocalSolver + MmAllocator (fast, single-pass)
Pipeline::iterative()                   // + MultiMarketSolver + fixed-point iteration
Pipeline::with_negrisk()                // + NegriskSolver for arbitrage
Pipeline::with_dual_decomposition()     // DualMaster + MultiMarketSolver (recommended)
Pipeline::full_platform()               // MILP + MWIS combination
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
    .multi_market_solver(MultiMarketSolver::new())
    .allocator(MmAllocator::new())
    .dual_master(DualMaster::new())
    .partial_solver(MilpSolver::with_timeout(1.0))
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
let pipeline = Pipeline::with_dual_decomposition();
let result = pipeline.solve(&problem);
// result.fills, result.prices, result.total_welfare
```

---

## Key Files Reference

| Component | File |
|-----------|------|
| Pipeline orchestration | `matching-solver/src/pipeline.rs` |
| Per-market clearing | `matching-solver/src/local_solver.rs` |
| Multi-market solver | `matching-solver/src/specialized/multi_market.rs` |
| Negrisk arbitrage | `matching-solver/src/specialized/negrisk.rs` |
| Dual decomposition | `matching-solver/src/dual_master.rs` |
| Budget allocation | `matching-solver/src/mm_allocator.rs` |
| MILP solver | `matching-solver/src/milp.rs` |
| Solution combination | `matching-solver/src/combiner/mod.rs` |
| MWIS algorithms | `matching-solver/src/combiner/mwis.rs` |
| Result validation | `matching-solver/src/verifier.rs` |
| Scenario generation | `matching-scenarios/src/scenario.rs` |
| CLI simulation | `matching-sim/src/main.rs` |

---

## Design Philosophy

1. **Modular Competition**: Solvers compete, MWIS selects best combination
2. **Welfare Maximization**: Objective is total user surplus, not platform extraction
3. **Fairness First**: FBA eliminates front-running, uniform clearing price protects LPs
4. **Constraint Separation**: Each solver handles specific constraints, clean interfaces
5. **Verifiability**: All results can be verified (ZK-proof compatible)
6. **Dual Approach**: Lagrangian relaxation provides principled handling of coupling constraints
