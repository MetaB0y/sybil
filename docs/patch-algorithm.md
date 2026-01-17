# Patch-Based Cross-Market Solving: Detailed Algorithm

## The Core Idea

**Base solution**: Solve each market independently (trivial, O(n log n) per market)
**Patches**: Small, local improvements that fill cross-market orders
**Combination**: Greedily apply non-conflicting patches

This is analogous to:
- MEV-boost bundles (patches = bundles)
- 2-approximation for metric TSP (greedy edge selection)
- Local search / hill climbing

---

## Definitions

### Market State
```
MarketState {
  markets: Map<MarketId, MarketClearing>
}

MarketClearing {
  price: Decimal,           // clearing price
  volume: Decimal,          // total matched volume
  fills: Map<OrderId, Fill> // which orders filled how much
}

Fill {
  amount: Decimal,
  price: Decimal  // always equals market clearing price in FBA
}
```

### Patch
```
Patch {
  id: PatchId,
  affected_markets: Set<MarketId>,
  order_fills: Map<OrderId, Fill>,      // new fills to add
  price_adjustments: Map<MarketId, Decimal>,  // price changes
  welfare_delta: Decimal,               // improvement over base
  solver_id: SolverId,
}
```

### Conflict
Two patches **conflict** if they share any affected market:
```
conflict(P1, P2) = P1.affected_markets ∩ P2.affected_markets ≠ ∅
```

---

## Algorithm: Patch Selection

### Naive Greedy (O(P²) where P = number of patches)

```python
def select_patches_greedy(patches: List[Patch]) -> List[Patch]:
    """Select non-conflicting patches maximizing total welfare."""
    # Sort by welfare delta descending
    patches.sort(key=lambda p: -p.welfare_delta)

    selected = []
    used_markets = set()

    for patch in patches:
        # Check conflict with already selected
        if patch.affected_markets & used_markets:
            continue  # Conflicts, skip

        # Accept this patch
        selected.append(patch)
        used_markets |= patch.affected_markets

    return selected
```

**Analysis**:
- This is greedy set packing
- Gives 1/2 approximation to optimal (for unweighted)
- For weighted: can be arbitrarily bad in worst case

### Better: Weighted Independent Set on Conflict Graph

**Conflict graph**:
- Nodes = patches
- Edges = conflicts
- Node weight = welfare_delta

**Problem**: Maximum Weight Independent Set (MWIS)

**Bad news**: MWIS is NP-hard in general

**Good news**: Our conflict graph has special structure!

### Exploiting Structure

**Observation**: Patches are "local" (affect ≤5 markets each)

**Conflict graph properties**:
- Maximum degree bounded by: (number of patches touching same market) × 5
- If patches are sparse, graph is sparse
- If markets are "clustered", graph has small treewidth

**Approach 1: Bounded-degree approximation**

For graphs with max degree Δ, greedy gives (Δ+1)/2 approximation.
If Δ is small (say ≤10), this is decent.

**Approach 2: Local search**

```python
def local_search_mwis(patches, iterations=1000):
    """Local search for MWIS."""
    selected = greedy_select(patches)  # Start with greedy

    for _ in range(iterations):
        # Try: remove one patch, add multiple non-conflicting
        improved = False
        for p in selected:
            candidates = [q for q in patches
                         if q not in selected
                         and not conflicts_with_any(q, selected - {p})]
            # Can we add more welfare by swapping?
            current_welfare = p.welfare_delta
            best_swap = find_best_subset(candidates, max_size=3)
            if sum(q.welfare_delta for q in best_swap) > current_welfare:
                selected = (selected - {p}) | set(best_swap)
                improved = True
                break

        if not improved:
            break

    return selected
```

**Approach 3: Randomized parallel**

Your idea: random permutations on many cores.

```python
def parallel_random_mwis(patches, num_cores=64, samples_per_core=100):
    """Run many random orderings in parallel, take best."""

    def random_greedy(patches):
        shuffled = random.shuffle(patches.copy())
        return greedy_select(shuffled), welfare(selected)

    # Run in parallel
    results = parallel_map(random_greedy, [patches] * (num_cores * samples_per_core))

    # Return best
    return max(results, key=lambda x: x[1])[0]
```

**Why this works**:
- Greedy order matters a lot
- Random sampling explores many orderings
- Embarrassingly parallel
- With enough samples, likely to find good solution

**Theoretical backing**: For MWIS, randomized greedy with good probability finds solution within constant factor of optimal (depends on graph structure).

---

## Algorithm: Full Pipeline

```python
def solve_batch(orderbook: Orderbook, solvers: List[Solver], time_budget_ms: int) -> Solution:
    """Main solving pipeline."""

    # Phase 1: Base solution (single-market)
    # Time: ~50ms for 5000 orders, 100 markets
    base = solve_base(orderbook)

    # Phase 2: Collect patches from solvers
    # Time: ~500ms (parallel, solver-dependent)
    deadline = now() + time_budget_ms * 0.6
    patches = collect_patches_parallel(orderbook, base, solvers, deadline)

    # Phase 3: Select best non-conflicting patches
    # Time: ~100ms
    selected = parallel_random_mwis(patches, num_cores=32, samples_per_core=50)

    # Phase 4: Apply patches to base
    # Time: ~50ms
    solution = apply_patches(base, selected)

    # Phase 5: Validate
    # Time: ~100ms
    assert validate_solution(solution, orderbook)

    return solution


def solve_base(orderbook: Orderbook) -> Solution:
    """Solve each market independently."""
    solution = Solution()

    for market_id, orders in orderbook.by_market().items():
        if market.is_binary():
            # Binary market: simple FBA
            clearing = solve_binary_fba(orders)
        else:
            # Multi-outcome: LP
            clearing = solve_multi_outcome_lp(orders)

        solution.set_market(market_id, clearing)

    return solution


def collect_patches_parallel(orderbook, base, solvers, deadline):
    """Collect patches from all solvers in parallel."""

    async def get_solver_patches(solver):
        try:
            return await solver.propose_patches(orderbook, base, deadline)
        except Timeout:
            return []

    # Run all solvers in parallel
    all_patches = await asyncio.gather(*[
        get_solver_patches(s) for s in solvers
    ])

    # Flatten and deduplicate
    patches = []
    seen = set()
    for solver_patches in all_patches:
        for p in solver_patches:
            patch_hash = hash_patch(p)
            if patch_hash not in seen:
                patches.append(p)
                seen.add(patch_hash)

    return patches
```

---

## Patch Generation (Solver Side)

### What makes a good patch?

A patch is valuable if:
1. Fills cross-market order(s) that base solution didn't
2. Welfare delta is positive (improves total surplus)
3. Affects few markets (less likely to conflict)

### Solver Strategy: Exhaustive Small Patches

```python
def generate_patches_exhaustive(orderbook, base) -> List[Patch]:
    """Generate all valuable 2-market patches."""
    patches = []

    # Get unfilled cross-market orders
    unfilled = [o for o in orderbook.cross_market_orders()
                if base.fill(o.id).amount < o.size]

    for order in unfilled:
        if len(order.markets) == 2:
            patch = try_fill_2market_order(order, base)
            if patch and patch.welfare_delta > 0:
                patches.append(patch)

    return patches


def try_fill_2market_order(order, base) -> Optional[Patch]:
    """Try to construct patch that fills a 2-market order."""
    m1, m2 = order.markets

    # Current prices
    p1 = base.price(m1)
    p2 = base.price(m2)

    # Order constraint: buy q1 in m1, sell q2 in m2, net cost ≤ budget
    # q1 * p1 - q2 * p2 ≤ budget
    # With atomic: q1 = q2 = q

    # Can we fill at current prices?
    if order.can_fill_at(p1, p2):
        # Yes! Patch just adds the fill
        return Patch(
            affected_markets={m1, m2},
            order_fills={order.id: Fill(order.size, ...)},
            price_adjustments={},  # No price change needed
            welfare_delta=compute_welfare_delta(order, base)
        )

    # No. Need to adjust prices.
    # Find minimal price adjustment that makes order fillable
    adjustment = find_minimal_adjustment(order, p1, p2, base)
    if adjustment is None:
        return None  # Can't fill this order

    # Check if adjustment improves welfare
    new_welfare = compute_welfare_with_adjustment(base, adjustment, order)
    if new_welfare <= base.welfare():
        return None  # Not worth it

    return Patch(
        affected_markets={m1, m2},
        order_fills={order.id: Fill(...)},
        price_adjustments=adjustment,
        welfare_delta=new_welfare - base.welfare()
    )
```

### Solver Strategy: JIT Liquidity Patches

```python
def generate_jit_patches(orderbook, base, mm_inventory) -> List[Patch]:
    """Generate JIT liquidity patches."""
    patches = []

    # Find unfilled orders that could match with JIT
    for order in orderbook.unfilled_in(base):
        # Can MM provide counterparty?
        jit_order = construct_jit_counterparty(order, mm_inventory)
        if jit_order is None:
            continue

        # Would this be profitable for MM?
        mm_profit = compute_mm_profit(jit_order, base)
        if mm_profit <= 0:
            continue

        # Welfare delta = user surplus + MM profit
        welfare_delta = order.surplus_if_filled(base) + mm_profit

        patches.append(Patch(
            affected_markets=order.markets,
            order_fills={order.id: Fill(...), jit_order.id: Fill(...)},
            price_adjustments=compute_new_prices(...),
            welfare_delta=welfare_delta,
        ))

    return patches
```

### Solver Strategy: Arbitrage Patches

```python
def generate_arb_patches(orderbook, base) -> List[Patch]:
    """Find and close arbitrage opportunities."""
    patches = []

    # Build price graph
    # Edge A->B with weight p means "A implies B with probability p"
    price_graph = build_price_graph(base)

    # Find inconsistencies
    for cycle in find_cycles(price_graph):
        if is_arbitrage(cycle):
            # Construct orders to close the arb
            arb_orders = construct_arb_orders(cycle)
            welfare_delta = compute_arb_welfare(arb_orders, base)

            patches.append(Patch(
                affected_markets=set(cycle.markets),
                order_fills=arb_orders,
                price_adjustments=...,
                welfare_delta=welfare_delta,
            ))

    return patches
```

---

## Applying Patches

```python
def apply_patches(base: Solution, patches: List[Patch]) -> Solution:
    """Apply selected patches to base solution."""
    solution = base.copy()

    for patch in patches:
        # Update prices
        for market_id, new_price in patch.price_adjustments.items():
            solution.set_price(market_id, new_price)

        # Add fills
        for order_id, fill in patch.order_fills.items():
            solution.add_fill(order_id, fill)

        # Recompute affected market clearings
        for market_id in patch.affected_markets:
            solution.recompute_clearing(market_id)

    return solution
```

**Critical**: After applying patches, must verify:
1. Markets still clear (buys = sells)
2. All fills respect order constraints
3. No budget violations
4. Prices are consistent

---

## Handling Conflicts: Detailed

### When patches overlap on a market

**Example**:
```
Patch A: Fill order X (markets 1,2), welfare +10
Patch B: Fill order Y (markets 2,3), welfare +8
Patch C: Fill order Z (markets 4,5), welfare +5

Conflict: A and B both touch market 2
No conflict: C is independent
```

**Greedy selects**: A (welfare 10), then C (welfare 5). Total: 15
**Optimal might be**: B (welfare 8) + something else

### The price consistency problem

If Patch A sets market 2 price to 0.52
And Patch B sets market 2 price to 0.48
They can't both apply.

**Solution**: Patches must be self-contained. A patch specifies:
- New prices for ALL affected markets
- These prices must be consistent with patch's fills
- When we select patches, we select non-overlapping price regimes

### Can we do better than greedy?

**ILP formulation**:
```
Variables:
  x_i ∈ {0,1} for each patch i (selected or not)

Objective:
  maximize Σ welfare_i × x_i

Constraints:
  For each market m:
    Σ_{patches touching m} x_i ≤ 1
```

This is set packing ILP. Can solve exactly for small instances (<100 patches).
For large instances, use LP relaxation + rounding.

**Practical approach**:
1. If <50 patches: solve ILP exactly
2. If 50-500 patches: LP relaxation + randomized rounding
3. If >500 patches: parallel random greedy

---

## Complexity Analysis

| Phase | Time Complexity | Practical Time (5000 orders) |
|-------|-----------------|------------------------------|
| Base solution | O(m × n log n) | ~50ms |
| Patch collection | O(solvers × solver_time) | ~500ms |
| MWIS selection | O(P² × iterations) | ~100ms |
| Apply patches | O(P × markets_per_patch) | ~50ms |
| Validation | O(n) | ~100ms |
| **Total** | | **~800ms** |

Leaves 200ms buffer in 1-second budget.

---

## Open Questions

1. **How many patches in practice?**
   - Depends on cross-market order density
   - Need simulation to estimate

2. **How much welfare do we lose vs optimal?**
   - Greedy gives 1/2 approx for unweighted
   - Weighted case is worse theoretically
   - But random parallel might be good in practice

3. **Can patches be "composed"?**
   - Patch A affects {1,2}, Patch B affects {2,3}
   - Could we merge into super-patch {1,2,3}?
   - This is re-solving, might be expensive

4. **Incremental patches?**
   - After selecting patches, are there new opportunities?
   - Run another round of patch collection?
   - Diminishing returns, probably 2 rounds max

---

## Comparison to MEV-Boost

| Aspect | MEV-Boost | Our System |
|--------|-----------|------------|
| Bundles/Patches | Tx sequences | Order fill sets |
| Conflict | Same tx in multiple bundles | Same market touched |
| Selection | Builder picks best block | MWIS on conflict graph |
| Composition | Complex (tx ordering) | Simpler (markets independent) |
| Objective | Builder profit | Welfare maximization |

Key difference: Our patches have CLEANER conflict structure (markets are discrete, either overlap or not). MEV bundles have messier conflicts (tx A might conflict with tx B in subtle ways).

This means our MWIS should be EASIER than MEV bundle selection.
