# KKT Analysis: Welfare Maximization in Prediction Markets

## The Optimization Problem

The matching engine solves a welfare-maximization problem over prices $p$ and fill quantities $q$:

```
max   W(p,q) = sum_i surplus_i(p) * q_i

s.t.
  (PB)  forall m: sum_{i in demand(m)} q_i = sum_{j in supply(m)} q_j + mint_m   [position balance]
  (UCP) forall i: q_i > 0  ==>  surplus_i(p) >= 0                                [uniform clearing price]
  (QTY) forall i: 0 <= q_i <= max_fill_i                                          [quantity bounds]
  (MM)  forall k: sum_{i in MM(k)} capital_i(p,q) <= B_k                          [MM budget]
  (GP)  forall g: sum_{m in g} p_m <= 1                                            [group price consistency]
  (MG)  forall g: group_mint_g >= 0                                                [group minting non-negative]
  (P)   forall m: 0 <= p_m <= 1                                                    [price bounds]
```

Where:
- `surplus_i(p) = limit_i - p_m(i)` for buyers, `p_m(i) - limit_i` for sellers
- `capital_i(p,q) = p * q` for BuyYes/SellNo, `(1-p) * q` for SellYes/BuyNo
- `mint_m` = per-market minting (YES+NO pair creation), cost = $1 per pair
- `group_mint_g` = group-level minting (one YES per market), cost = $1 per set

## Minting Economics

### Per-Market Minting (mint_m)

Creates one YES share + one NO share for cost $1. Revenue at prices p:
```
revenue = p_YES * Q + p_NO * Q = (p_YES + p_NO) * Q = $1 * Q
```

**Always zero net cost** because p_YES + p_NO = $1 by complementarity. Per-market minting provides unlimited supply at any price without changing the objective. MILP exploits this to push prices to extremes (e.g., 0% YES) and fill all demand.

### Group Minting (group_mint_g)

Creates one YES share per market in a mutually exclusive group. Cost = $1 per set.

**When Σp ≥ $1**: Revenue = Σ p_m ≥ $1. This is a profitable arbitrage (negrisk). No subsidy needed. The heuristic solver performs this.

**When Σp < $1**: Revenue = Σ p_m < $1. **This requires a subsidy of $1 - Σp per set.** The protocol must absorb the loss. This is NOT sound for a trustless protocol. MILP does this implicitly (the cost appears in its objective as `minting_cost`), but it represents a real economic cost that somebody must pay.

### Why Sub-$1 Group Minting Doesn't Help the Heuristic Anyway

Investigation of the small preset revealed: **there are zero unfilled buy-YES orders with limit ≥ clearing price** on underperforming groups. The LocalSolver already filled ALL eligible demand — the remaining ~20 unfilled orders per market all have limits below the clearing price.

The only way to unlock more welfare is to **change the prices themselves**, not just add supply at current prices.

## Lagrangian (with Minting)

Relaxing position balance (PB), MM budget (MM), and group price consistency (GP):

```
L = W(p,q) - sum_m nu_m * mint_m - sum_g rho_g * group_mint_g
  + sum_m alpha_m * (D_m - S_m - mint_m - group_contribution_m)
  + sum_k mu_k * (B_k - C_k(p,q))
  + sum_g gamma_g * (1 - sum_{m in g} p_m)
```

Where:
- `alpha_m` = position balance dual (shadow price of supply on market m)
- `mu_k >= 0` = MM budget dual
- `gamma_g >= 0` = group price consistency dual (active when Σp = $1)
- `nu_m` = $1 (per-market minting cost)
- `rho_g` = $1 (group minting cost)

### KKT for per-market minting

```
dL/d(mint_m) = -$1 + alpha_m_YES + alpha_m_NO = 0
```

At optimum: `alpha_m_YES + alpha_m_NO = $1`. The shadow prices of YES and NO supply always sum to $1. This is why per-market minting is free — the value of creating supply equals the cost.

### KKT for group minting

```
dL/d(group_mint_g) = -$1 + sum_{m in g} alpha_m_YES <= 0   (= 0 if group_mint_g > 0)
```

Group minting is active when `sum alpha_m_YES >= $1` — the total shadow price of YES supply across the group exceeds the minting cost.

## KKT Stationarity: dL/dp_m = 0

```
dL/dp_m = dW/dp_m + alpha_m * d(D-S)/dp_m - sum_k mu_k * dC_k/dp_m - gamma_g = 0
```

### Term 1: dW/dp_m = -(D_m - S_m) at fill quantities

### Term 2: alpha_m * d(D-S)/dp_m — standard tatonnement force

### Term 3: -sum_k mu_k * dC_k/dp_m — MM budget bias

For BuyYes on market m: `dC_k/dp = q_i` (price increase costs more capital)
For SellYes on market m: `dC_k/dp = -q_i` (price increase uses less capital)

### Term 4: -gamma_g — group price consistency pressure

When gamma_g > 0 (Σp = $1 binding), this pushes all market prices in the group DOWN. Combined with position balance, this creates a force toward negrisk-optimal prices.

## Three Sources of Heuristic Suboptimality

### 1. Missing MM Budget Term (mu_k)

Tatonnement finds D(p) = S(p), ignoring the capital constraint. The optimal prices should be shifted:

```
Delta_p_m ~ -mu_k * (sum_{MM buys YES} q_i - sum_{MM sells YES} q_i)
```

Markets where MMs buy YES → price shifts DOWN (cheaper capital per unit).
Markets where MMs sell YES → price shifts UP.

**Impact**: On small preset, MILP uses 95.5% of MM budget vs Dual's 22.1%. The greedy knapsack can only select fills that satisfy at the heuristic's clearing prices, which are wrong.

### 2. Lambda Shading is Structurally Broken

DualMaster uses lambda to enforce Σp = $1 by shading order limits. Investigation revealed this is **self-defeating**:

1. Lambda shading inflates effective buyer limits → LocalSolver finds higher clearing price
2. But fills are checked against **original** limits: `order.is_satisfied_at_price(fill_price)`
3. The higher clearing price exceeds original limits → all new crossings rejected
4. Zero fills → prices unchanged → lambda grows uselessly

Tested with λ = -1.77 (177c shading!) over 50 iterations: **zero additional fills, G2 residual stuck at -13.0%**. The mechanism cannot close group price gaps.

**Root cause**: Shading changes the demand/supply curves, but the resulting equilibrium price overshoots the original limits. The gap between shaded limit and original limit grows linearly with lambda, while the price adjustment needed is the same — so the overshoot gets worse with more shading.

### 3. No Price Space Exploration

The heuristic finds LOCAL equilibrium prices (where S=D per market). MILP can explore radically different price vectors:

- Push some markets to 0% YES price (all buy orders fill, supply from minting)
- Push group sums to exactly $1 for optimal minting
- Jointly optimize prices and fill quantities

On the small preset, MILP sets G0M2 and G1M1 to **0%** clearing price — a solution the heuristic can never find because there's zero natural supply at 0%.

## Decomposition Structure

### The problem decomposes by group when MM budgets and bundles are relaxed

Without MM constraints and without cross-group bundles, each market group is an independent subproblem:

```
Per-group subproblem (K markets):
  max  Σ_k W_k(p_k)  -  $1 × group_mint
  s.t. Σ p_k + slack = $1   (slack ≥ 0)
       position balance per market (demand = supply + mint_k + group_mint)
       mint_k ≥ 0, group_mint ≥ 0
```

Per-market minting (mint_k) is free (p + (1-p) = $1), so it just ensures position balance.
Group minting provides supply to ALL markets simultaneously for $1.

### Key mathematical insight: optimal group prices satisfy equal marginal welfare

At optimum on the Σp = $1 simplex:
```
dW_k/dp_k = -excess_demand_k(p_k) = λ   for all k in group
```

All markets in the group have the SAME excess demand. This means a single scalar λ determines all prices. Binary search on λ finds the solution:

1. For each candidate λ: p_k(λ) = price where excess_demand_k = λ (from order book)
2. Check if Σ p_k(λ) = $1
3. Binary search on λ

This is O(K × N × log(N) × log(precision)) per group. Very fast.

### Limitation of 1D parametric search

The parametric search adds UNIFORM supply to all markets (same λ). But the optimal solution may require REDISTRIBUTING prices within the group (lower one market, raise another).

Example: MILP sets one market to 0%, concentrating demand there while raising other markets' prices. The parametric search can't do this because it only moves all prices in the same direction.

**Fix: coordinate descent on the simplex after parametric search.** For each pair of markets (i,j), try transferring price from i to j. O(K² × N × iters) per group — fast for K=5.

### Coupling constraints require Lagrangian relaxation

- **MM budgets** couple orders across different groups → relax via mu duals
- **Bundle orders** span multiple markets/groups → relax via Lagrangian or handle post-hoc
- **Outer loop**: update mu based on budget violations
- **Inner loop**: per-group exact optimization with mu-adjusted surplus

```
Per-group subproblem (given mu):
  max  Σ_k W_k(p_k, mu)  -  $1 × group_mint
  where W_k includes mu-adjusted surplus for MM orders:
    effective_surplus = surplus - mu_k × capital_per_unit
```

This is the standard Lagrangian decomposition: relax coupling constraints, solve independent subproblems, coordinate via dual updates.

## Proposed Architecture: Joint Group Solver

Replace the sequential pipeline (LocalSolver → DualMaster → MmAllocator → group_minting) with a single unified algorithm:

```
fn solve(problem) -> Result:
  // 1. Build per-market order books (PrecomputedMarket)
  // 2. Initialize mu = 0 for all MM constraints

  for outer_iter in 0..max_outer:
    // 3. Per-group joint optimization (the core)
    for each group:
      prices[group] = parametric_search(group, orders, mu)
      prices[group] = coordinate_descent(group, orders, mu, prices[group])

    // 4. Standalone markets: standard clearing (λ=0 case)
    for each standalone market:
      prices[m] = local_solver_clearing(m, orders)

    // 5. Fill extraction at joint-optimal prices
    fills = extract_fills(orders, prices, mm_tracker)

    // 6. Bundle fills
    bundle_fills = extract_bundle_fills(orders, prices)

    // 7. MM dual update
    for each mm_constraint k:
      usage = compute_capital_usage(fills, k)
      mu[k] = max(0, mu[k] + lr * (usage - budget) / budget)

    // 8. Check convergence
    if all MM budgets satisfied within tolerance:
      break

  // 9. enforce_ucp (light — prices already correct)
  enforce_ucp(fills, prices)
```

### What this replaces

| Current component | Replaced by |
|---|---|
| LocalSolver (per-market) | Parametric search with λ=0 for standalone markets |
| DualMaster (lambda shading) | Parametric search on Σp=$1 simplex |
| MmAllocator (greedy knapsack) | Lagrangian mu duals in price optimization |
| group_minting (water-filling) | Built into parametric search (Q IS group minting) |
| simplex_search (binary on Q) | IS the parametric search (promoted from post-processing to primary) |
| simulate_enforce_ucp gate | Unnecessary — prices are correct by construction |

### Complexity

- Per group: O(K × N × log(N) × log(range)) for parametric search + O(K² × N² × cd_iters) for coordinate descent
- Outer iterations: O(max_outer) ≈ 10-20 for MM convergence
- Total: dominated by O(max_outer × G × K² × N²) where G = groups, K = markets/group, N = orders/market
- For small preset (G=3, K=5, N=60): ~10 × 3 × 25 × 3600 = 2.7M ops. Sub-millisecond.

### Why this should close the gap

1. **Group price consistency**: built into price discovery (parametric search on Σp=$1), not post-hoc patching
2. **MM budget optimization**: joint via mu duals affecting price discovery, not greedy post-hoc
3. **Price space exploration**: coordinate descent can find non-obvious price redistributions within groups
4. **Minting**: group minting = parametric search Q parameter; per-market minting = free (implicit)
5. **Clean**: one algorithm with mathematical grounding (Lagrangian decomposition)

## Summary

| Source of Gap | Magnitude (small) | Can Heuristic Fix? | Joint Solver Approach |
|---------------|-------------------|-------------------|----------------------|
| Missing mu_k (MM budget) | ~$470 (46%) | Yes | Lagrangian mu duals in price finding |
| Lambda shading broken | ~$350 (34%) | Yes | Parametric search replaces shading |
| Price space (0% prices, minting) | ~$200 (20%) | Partially | Coordinate descent on simplex |

| Solver Property | Current Pipeline | Joint Group Solver | MILP |
|-----------------|-----------------|-------------------|------|
| Price finding | Per-market independent | Per-group joint (simplex) | Global optimal |
| MM budget | Post-hoc greedy | Lagrangian mu duals | Jointly optimal |
| Group prices | Lambda (broken) | Parametric search (exact) | Jointly optimal |
| Minting | Post-hoc water-filling | Built into price search | Jointly optimal |
| Bundles | Post-hoc | Lagrangian or post-hoc | Jointly optimal |
| Speed | ~1ms | ~1ms (expected) | ~5s |
| Soundness | Full | Full | Needs minting_cost tracking |
