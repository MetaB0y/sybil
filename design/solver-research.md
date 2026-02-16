# Solver Research: Closing the MILP-Heuristic Gap

This document consolidates research on why MILP outperforms heuristic solvers and three approaches to closing the gap.

---

## 1. The Gap

MILP beats all heuristic solvers by 4-5x on realistic scenarios. The gap comes from three sources:

| Source | Magnitude (small preset) | Root Cause |
|--------|--------------------------|------------|
| Missing MM budget term | ~$470 (46%) | Heuristic ignores capital constraints during price discovery |
| Lambda shading broken | ~$350 (34%) | DualMaster's bid shading overshoots original limits |
| No price space exploration | ~$200 (20%) | Heuristic finds local equilibria, can't reach 0% prices |

### What MILP Does That Heuristics Can't

**Group-level minting**: MILP has an explicit `group_mint_g` variable. For a group of N mutually exclusive outcomes, one unit of group mint creates 1 YES share on every market for $1. This is N times cheaper per YES share than per-market minting ($1 for 1 YES + 1 NO).

This lets MILP fill buy-YES orders **that have no counterparty** — it creates virtual supply from the group structure. It can set clearing prices to 0% on low-demand markets because the synthetic supply doesn't constrain the price.

**Concrete example**: 3-candidate election with only buy-YES orders: A@40c, B@35c, C@30c. No sellers.
- Heuristic: no supply → no fills → $0 welfare
- MILP: `group_mint = 100` → fills all 3 → welfare = $5 (from the $0.05/unit arbitrage between individual limits and the $1 group cost)

### Why Each Heuristic Phase Fails

- **LocalSolver**: Clears per-market. Without sell orders, no fills. No concept of cross-market supply.
- **NegriskSolver**: Creates demand (arb orders buying), not supply. Volume bottlenecked by `min(existing_fills)` — if any market has 0 fills, 0 arb.
- **DualMaster**: Lambda shading adjusts willingness-to-pay but can't create supply. Investigation showed it's self-defeating: shading inflates effective limits → higher clearing price → exceeds original limits → all new crossings rejected → zero additional fills even after 50 iterations.
- **MmAllocator**: Post-hoc greedy knapsack at wrong prices. MILP uses 95.5% of MM budget vs Dual's 22.1%.

---

## 2. Mathematical Foundation (KKT Analysis)

### The Optimization Problem

```
max   W(p,q) = Σ surplus_i(p) * q_i

s.t.
  (PB)  ∀m: Σ demand_m = Σ supply_m + mint_m + group_mint_g   [position balance]
  (UCP) ∀i: q_i > 0 ⟹ surplus_i(p) ≥ 0                      [uniform clearing price]
  (QTY) ∀i: 0 ≤ q_i ≤ max_fill_i                              [quantity bounds]
  (MM)  ∀k: Σ capital_i(p,q) ≤ B_k                            [MM budget]
  (GP)  ∀g: Σ p_m ≤ 1                                          [group price consistency]
```

### Key KKT Conditions

**Per-market minting**: `α_YES + α_NO = $1` at optimum. Shadow prices of YES and NO supply always sum to $1. Per-market minting is free — the value of creating supply equals the cost.

**Group minting**: Active when `Σ α_m_YES ≥ $1` — total shadow price of YES supply across the group exceeds the minting cost.

**Price stationarity** (`dL/dp_m = 0`):
```
dW/dp + α · d(D-S)/dp - Σ μ_k · dC_k/dp - γ_g = 0
```

The γ_g term (group price consistency pressure) pushes all market prices down when Σp = $1 is binding. Combined with position balance, this creates forces toward negrisk-optimal prices. The μ_k term (MM budget) shifts prices on markets where MMs are active — markets where MMs buy YES should have lower prices (cheaper capital per unit).

### Decomposition Structure

Without MM constraints and cross-group bundles, each market group is independent:
```
Per-group: max Σ W_k(p_k) - $1 × group_mint
s.t.       Σ p_k + slack = $1, position balance per market
```

At optimum on the Σp = $1 simplex, all markets in a group have the **same excess demand** (equal marginal welfare). A single scalar λ determines all prices via binary search — O(K × N × log(N) × log(precision)) per group.

---

## 3. Approach A: Water-Filling Group Minting

A targeted fix for the dominant gap source (group minting).

### Algorithm

For a group G with markets {m₁, ..., mₙ}, find optimal group mint quantity Q*:

```
for each market m in G:
    demands[m] = sorted unfilled buy-YES limits (descending)

Q* = largest Q where Σ_m demands[m][Q] ≥ $1
```

Keep minting as long as the sum of marginal buyer limits across all markets exceeds $1. Complexity: O(D log D).

### Clearing Prices

Set p_m = L_{m,Q*} (the marginal buyer's limit on each market). This satisfies UCP: filled buyers are above marginal, unfilled below.

### Position Balance

Group minting creates YES without NO. Two options:
- **Arb orders** (NegriskSolver convention): Create synthetic sell-YES orders as counterparties. Works with existing verifier.
- **Verifier-level**: Add `group_mint: HashMap<GroupId, u64>` to BlockWitness. Cleaner for ZK proofs but requires verifier changes.

### Integration

Runs after LocalSolver on residual unfilled demand. Best inside DualMaster iteration loop (benefits from price convergence). Expected to reduce gap from 4-5x to 1.5-2x.

---

## 4. Approach B: Joint Group Solver

Replace the sequential pipeline with a unified algorithm based on Lagrangian decomposition.

### Architecture

```
for outer_iter in 0..max_outer:
    // Per-group joint optimization (parametric search on Σp=$1 simplex)
    for each group:
        prices[group] = parametric_search(group, orders, mu)
        prices[group] = coordinate_descent(group, orders, mu, prices[group])

    // Standalone markets: standard clearing
    for each standalone market:
        prices[m] = local_solver_clearing(m, orders)

    // Fill extraction + bundle fills
    fills = extract_fills(orders, prices, mm_tracker)

    // MM dual update
    for each mm_constraint k:
        mu[k] = max(0, mu[k] + lr * (usage - budget) / budget)

    if all MM budgets satisfied: break

enforce_ucp(fills, prices)
```

### What This Replaces

| Current | Joint Solver |
|---------|-------------|
| LocalSolver (per-market) | Parametric search with λ=0 for standalone markets |
| DualMaster (lambda shading) | Parametric search on Σp=$1 simplex |
| MmAllocator (greedy knapsack) | Lagrangian μ duals in price optimization |
| group_minting (water-filling) | Built into parametric search (Q IS group minting) |
| simulate_enforce_ucp gate | Unnecessary — prices correct by construction |

### Complexity

Dominated by O(max_outer × G × K² × N²). For small preset (G=3, K=5, N=60): ~2.7M ops, sub-millisecond.

### Limitation

Coordinate descent on the simplex can redistribute prices within a group but may not find all non-obvious redistributions. MILP can jointly optimize across all groups simultaneously.

---

## 5. Approach C: Smoothed Batch Auction (Entropy Smoothing)

A fundamentally different approach using gradient-based optimization.

### Key Insight

The welfare landscape is **piecewise-linear** — flat plateaus with kinks at order limit prices. Not convex, not concave, gradient zero almost everywhere. Direct gradient descent fails.

**Fix**: Add entropy regularization to smooth the landscape:

```
W_ε(prices) = max over fills:
    Σ (limit_i - price_i) · q_i + ε · Σ H(q_i / Q_i)
    s.t. position balance, 0 ≤ q_i ≤ Q_i
```

where H is binary entropy and ε is temperature. This makes fills a smooth sigmoid function of surplus, and the welfare landscape becomes differentiable.

### Algorithm: Gradient Descent + Annealing

```
Initialize prices (uniform within groups)
ε = ε_start (e.g., 0.1)

Repeat until ε < ε_min:
    # Inner loop: gradient descent at current temperature
    For each group: gradient step + project onto simplex
    Update bundle dual variables (λ_b += step × violation)
    Update MM dual variables (μ_j += step × violation)

    ε *= cooling_factor (e.g., 0.5)

Round fills to integers, enforce constraints, verify
```

### Why It Works

- **High ε**: Smooth landscape, one broad peak, gradient descent finds it
- **Gradually lower ε**: Peak sharpens but we track it (warm start)
- **Final rounding**: At ε ≈ 0, fills are nearly 0-or-max, rounding is clean

This is **deterministic annealing** — simulated annealing's exploration (via temperature) with gradient descent's efficiency (via gradient information).

### Connection to LMSR

Hanson's Logarithmic Market Scoring Rule is exactly the entropy-regularized single-market-maker version. Our multi-order batch auction with smoothing is the generalization to multiple agents.

### Lagrangian Decomposition

At fixed dual variables (λ for bundles, μ for MMs), the problem **decomposes by group**. Each group subproblem is small (K-dimensional simplex). Number of dual variables = |bundles| + |MMs|, not exponential. Convergence: O(1/√T) with subgradient, O(1/T²) with Nesterov acceleration. The duality gap provides a **quality certificate** (provable bound on distance from optimal).

### Open Questions

1. Position balance with soft fills — Lagrangian relaxation or project onto balanced manifold?
2. Step size selection — fixed, diminishing, or adaptive (Adam)?
3. Integer rounding — can violate constraints, needs careful repair
4. Interaction between smoothing and Lagrangian relaxation (theory says composable, needs empirical validation)

---

## 6. Comparison of Approaches

| Property | Water-Filling (A) | Joint Group (B) | Smoothed (C) | MILP |
|----------|-------------------|-----------------|--------------|------|
| Scope | Group minting only | Group prices + MM + minting | All constraints | All constraints |
| Implementation | Additive phase | Replace pipeline | Replace pipeline | Existing |
| Price finding | Uses LocalSolver | Parametric search | Gradient descent | Global optimal |
| MM budget | Separate (MmAllocator) | Lagrangian μ duals | Lagrangian μ duals | Jointly optimal |
| Group prices | N/A | Parametric (exact) | Simplex projection | Jointly optimal |
| Minting | Explicit water-fill | Built into search | Implicit via smoothing | Jointly optimal |
| Bundles | Separate | Lagrangian or post-hoc | Lagrangian λ duals | Jointly optimal |
| Expected gap | 1.5-2x | ~1.2x | ~1.1x (theory) | 1x (optimal) |
| Speed | ~1ms | ~1ms | ~10ms (estimate) | ~5s |
| Risk | Low (additive) | Medium (replacement) | High (new approach) | N/A |

### Recommended Path

1. **Start with A** (water-filling): Low risk, captures dominant gap source
2. **Then B** (joint group solver): Principled replacement, addresses all three gap sources
3. **Evaluate C** (smoothed): If B's coordinate descent isn't enough, smoothing provides a fundamentally different exploration strategy
4. **Keep MILP** as gold standard benchmark throughout

---

## References

- Budish, Cramton, Shim (2015). The High-Frequency Trading Arms Race.
- Chen & Pennock (2007). A Utility Framework for Bounded-Loss Market Makers.
- Fisher (1981). The Lagrangian Relaxation Method for Solving Integer Programming Problems.
- Hanson (2003). Combinatorial Information Market Design.
- Nesterov (2005). Smooth Minimization of Non-Smooth Functions.
- Boyd et al. (2011). Distributed Optimization and Statistical Learning via ADMM.
