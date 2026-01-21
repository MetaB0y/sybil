# Matching Algorithm

## Overview

The matching algorithm solves cross-market order matching using a **patch-based** approach:

1. **Base solution**: Solve each market independently (trivial, O(n log n))
2. **Patches**: Local improvements that fill cross-market orders
3. **Combination**: Select non-conflicting patches via MWIS

This is analogous to MEV-boost bundles or local search optimization.

---

## Definitions

### Market State
```rust
struct MarketClearing {
    price: Nanos,                    // Clearing price
    volume: Qty,                     // Total matched volume
    fills: HashMap<OrderId, Fill>,   // Order fills
}

struct Solution {
    markets: HashMap<MarketId, MarketClearing>,
    total_welfare: i64,
}
```

### Patch
```rust
struct Patch {
    affected_markets: HashSet<MarketId>,
    order_fills: HashMap<OrderId, Fill>,
    price_adjustments: HashMap<MarketId, Nanos>,
    welfare_delta: i64,
    solver_id: SolverId,
}
```

### Conflict Rule
Two patches **conflict** if they share any affected market:
```
conflict(P1, P2) = P1.affected_markets ∩ P2.affected_markets ≠ ∅
```

---

## Algorithm: Patch Selection

### Greedy Selection

```rust
fn select_patches_greedy(patches: &[Patch]) -> Vec<&Patch> {
    // Sort by welfare delta descending
    let mut sorted: Vec<_> = patches.iter().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.welfare_delta));

    let mut selected = Vec::new();
    let mut used_markets = HashSet::new();

    for patch in sorted {
        // Check conflict with already selected
        if patch.affected_markets.is_disjoint(&used_markets) {
            selected.push(patch);
            used_markets.extend(&patch.affected_markets);
        }
    }

    selected
}
```

**Analysis**:
- O(P log P) for sorting, O(P × M) for selection (P patches, M markets per patch)
- Greedy gives 1/2 approximation for unweighted set packing
- Works well when patches are sparse (most are)

### Randomized Parallel MWIS

For better solutions, run random orderings in parallel:

```rust
fn parallel_random_mwis(patches: &[Patch], iterations: usize) -> Vec<&Patch> {
    (0..iterations)
        .into_par_iter()
        .map(|_| {
            let mut shuffled: Vec<_> = patches.iter().collect();
            shuffled.shuffle(&mut thread_rng());
            greedy_select(&shuffled)
        })
        .max_by_key(|selected| selected.iter().map(|p| p.welfare_delta).sum::<i64>())
        .unwrap()
}
```

**Why this works**:
- Greedy order matters significantly
- Random sampling explores many orderings
- Embarrassingly parallel
- With enough samples, finds good solutions

---

## Full Pipeline

```rust
fn solve_batch(problem: &Problem, solvers: &[&dyn Solver]) -> MatchingResult {
    // Phase 1: Base solution (single-market)
    let base = solve_base(problem);

    // Phase 2: Collect patches from solvers
    let patches: Vec<Patch> = solvers
        .par_iter()
        .flat_map(|solver| solver.propose_patches(problem, &base))
        .collect();

    // Phase 3: Select best non-conflicting patches
    let selected = parallel_random_mwis(&patches, 1000);

    // Phase 4: Apply patches to base
    let solution = apply_patches(base, &selected);

    // Phase 5: Validate
    assert!(validate_solution(&solution, problem));

    solution
}
```

### Phase 1: Base Solution

Solve each market independently using standard FBA:

```rust
fn solve_base(problem: &Problem) -> Solution {
    let mut solution = Solution::new();

    for market in &problem.markets {
        let orders: Vec<_> = problem.orders
            .iter()
            .filter(|o| o.is_single_market() && o.markets[0] == market.id)
            .collect();

        let clearing = solve_single_market(market, &orders, &problem.liquidity);
        solution.set_market(market.id, clearing);
    }

    solution
}
```

For binary markets, this is a simple supply/demand intersection.

---

## Patch Generation Strategies

### Strategy 1: Exhaustive Small Patches

Generate all valuable 2-market patches:

```rust
fn generate_2market_patches(problem: &Problem, base: &Solution) -> Vec<Patch> {
    let mut patches = Vec::new();

    // Find unfilled cross-market orders
    for order in &problem.orders {
        if order.num_markets != 2 { continue; }
        if base.is_filled(order.id) { continue; }

        if let Some(patch) = try_fill_order(order, base, problem) {
            if patch.welfare_delta > 0 {
                patches.push(patch);
            }
        }
    }

    patches
}

fn try_fill_order(order: &Order, base: &Solution, problem: &Problem) -> Option<Patch> {
    let m1 = order.markets[0];
    let m2 = order.markets[1];

    let p1 = base.price(m1);
    let p2 = base.price(m2);

    // Check if fillable at current prices
    if order.can_fill_at(p1, p2) {
        return Some(Patch {
            affected_markets: hashset![m1, m2],
            order_fills: hashmap![order.id => compute_fill(order, p1, p2)],
            price_adjustments: HashMap::new(),
            welfare_delta: compute_welfare(order, p1, p2),
            solver_id: SolverId::CrossMarket,
        });
    }

    // Try minimal price adjustment
    find_adjustment_patch(order, base, problem)
}
```

### Strategy 2: Arbitrage Patches

Find and close cross-market mispricings:

```rust
fn generate_arb_patches(problem: &Problem, base: &Solution) -> Vec<Patch> {
    let mut patches = Vec::new();

    // Build implication graph from market correlations
    let graph = build_implication_graph(problem);

    // Find price inconsistencies
    for (m1, m2, implied_relation) in graph.edges() {
        let p1 = base.price(m1);
        let p2 = base.price(m2);

        let implied_p2 = implied_relation.apply(p1);
        let mispricing = implied_p2 - p2;

        if mispricing.abs() > MIN_ARB_THRESHOLD {
            patches.push(create_arb_patch(m1, m2, mispricing, base));
        }
    }

    patches
}
```

---

## Applying Patches

```rust
fn apply_patches(mut base: Solution, patches: &[&Patch]) -> Solution {
    for patch in patches {
        // Update prices
        for (&market_id, &new_price) in &patch.price_adjustments {
            base.set_price(market_id, new_price);
        }

        // Add fills
        for (&order_id, fill) in &patch.order_fills {
            base.add_fill(order_id, fill.clone());
        }
    }

    base
}
```

**Post-application validation**:
1. Markets still clear (buys = sells)
2. All fills respect order constraints
3. No budget violations
4. Prices are consistent

---

## Complexity Analysis

| Phase | Time Complexity | Typical Time (5000 orders) |
|-------|-----------------|----------------------------|
| Base solution | O(m × n log n) | ~20ms |
| Patch collection | O(solvers × cross_orders) | ~100ms |
| MWIS selection | O(P × iterations) | ~50ms |
| Apply patches | O(P × markets_per_patch) | ~10ms |
| Validation | O(n) | ~20ms |
| **Total** | | **~200ms** |

---

## Conflict Graph Structure

Patches conflict when they touch the same market. The conflict graph:
- Nodes = patches
- Edges = shared markets
- Weight = welfare_delta

**Key property**: Patches are "local" (affect 2-4 markets each), so the conflict graph is sparse.

For sparse graphs:
- Maximum degree is bounded
- Greedy achieves better approximation ratios
- MWIS is easier than general case

---

## Comparison to MEV-Boost

| Aspect | MEV-Boost | Our System |
|--------|-----------|------------|
| Bundles/Patches | Tx sequences | Order fill sets |
| Conflict | Same tx in bundles | Same market touched |
| Selection | Builder picks block | MWIS on conflict graph |
| Objective | Builder profit | Welfare maximization |

Key difference: Our patches have **cleaner conflict structure** (markets are discrete, either overlap or not). MEV bundles have messier conflicts (subtle tx interactions).

This makes our MWIS **easier** than MEV bundle selection.

---

## Implementation Notes

The current implementation in `matching-solver/src/combiner/`:

- `ConflictGraph` - Adjacency list representation
- `MwisSolver` - Greedy and randomized algorithms
- `SolutionCombiner` - Full combination pipeline

See `pipeline.rs` for the pipeline that orchestrates the solving phases.
