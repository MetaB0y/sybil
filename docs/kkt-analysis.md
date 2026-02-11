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

## Smoothed Gradient: A Better Platform?

The smoothed gradient solver adjusts prices directly via excess demand gradient, without the original-limit filter problem. This makes it naturally suitable for:

### Group Arb Pressure

Add a penalty term to the price gradient:

```
gradient_m += K * ($1 - sum_{m' in group(m)} p_m')   for each m in a group
```

This pushes group sums toward $1 directly through price adjustment, avoiding the broken lambda-shading mechanism. The force is:
- Positive (push price UP) when Σp < $1
- Negative (push price DOWN) when Σp > $1
- Zero at Σp = $1 (equilibrium)

Mathematically, this is equivalent to adding a quadratic penalty:
```
Penalty = -K/2 * sum_g (sum_{m in g} p_m - $1)^2
```

### MM Budget in Gradient

Embed the MM budget dual directly:

```
p_m <- p_m + lr * (excess_demand_m - sum_k mu_k * dC_k/dp_m + K * group_residual_m)
mu_k <- max(0, mu_k + lr_mu * (C_k - B_k))
```

This is a primal-dual method that jointly handles:
- Market clearing (excess demand = 0)
- MM budget optimality (mu * dC/dp term)
- Group price consistency (penalty term)

### Limitation

Even with these improvements, the smoothed solver still can't push prices to 0% (zero natural supply). That requires minting as a supply source, which changes the problem structure fundamentally. The smoothed solver could include per-market minting in its supply curves, but this makes the excess demand calculation degenerate (infinite supply → any price is an equilibrium).

## Summary

| Source of Gap | Magnitude (small) | Can Heuristic Fix? |
|---------------|-------------------|-------------------|
| Missing mu_k (MM budget) | ~$470 (46%) | Yes — embed in gradient |
| Lambda shading broken | ~$350 (34%) | Yes — use gradient, not shading |
| Price space (0% prices, minting) | ~$200 (20%) | Hard — requires global search |

| Solver Property | Tatonnement | DualMaster | Smoothed+fixes | MILP |
|-----------------|-------------|------------|----------------|------|
| Price finding | D=S per market | D=S + broken λ | D=S + group penalty + μ | Global optimal |
| MM budget | Post-hoc greedy | Post-hoc greedy | Built into gradient | Jointly optimal |
| Group prices | Ignored | Lambda (broken) | Penalty term | Jointly optimal |
| Minting | None | Group (Σp≥$1 only) | Group (Σp≥$1 only) | Per-market + group |
| Soundness | Full | Full | Full | Needs minting_cost tracking |

The gap between heuristic and MILP is smallest (~0%) on large problems where natural supply/demand depth dominates, and largest (~36%) on small problems where minting-based price exploration matters most.
