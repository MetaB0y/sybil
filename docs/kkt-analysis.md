# KKT Analysis: Welfare Maximization with MM Budget Constraints

## The Optimization Problem

The matching engine solves a welfare-maximization problem over prices $p$ and fill quantities $q$:

```
max   W(p,q) = sum_i surplus_i(p) * q_i

s.t.
  (PB)  forall m: sum_{i in demand(m)} q_i = sum_{j in supply(m)} q_j     [position balance]
  (UCP) forall i: q_i > 0  ==>  surplus_i(p) >= 0                        [uniform clearing price]
  (QTY) forall i: 0 <= q_i <= max_fill_i                                  [quantity bounds]
  (MM)  forall k: sum_{i in MM(k)} capital_i(p,q) <= B_k                  [MM budget]
  (P)   forall m: 0 <= p_m <= 1                                           [price bounds]
```

Where:
- `surplus_i(p) = limit_i - p_m(i)` for buyers (happy when clearing price is below their limit)
- `surplus_i(p) = p_m(i) - limit_i` for sellers (happy when clearing price is above their limit)
- `capital_i(p,q) = p * q` for BuyYes/SellNo orders
- `capital_i(p,q) = (1-p) * q` for SellYes/BuyNo orders

This is a mixed-integer bilinear program (welfare is linear in q for fixed p, but capital constraints couple p and q). MILP solves it directly; heuristic solvers approximate.

## Lagrangian

Relaxing position balance (PB) and MM budget (MM) constraints:

```
L(p, q, lambda, mu) = W(p,q) + sum_m lambda_m * (D_m(p,q) - S_m(p,q)) + sum_k mu_k * (B_k - C_k(p,q))
```

Where:
- `lambda_m` is the dual variable for position balance on market m
- `mu_k >= 0` is the dual variable for MM budget constraint k
- `D_m - S_m` is excess demand on market m
- `C_k(p,q) = sum_{i in MM(k)} capital_i(p,q)` is total capital used by MM k

## KKT Stationarity: dL/dp_m = 0

Taking the derivative with respect to price p_m:

```
dL/dp_m = dW/dp_m + lambda_m * d(D_m - S_m)/dp_m - sum_k mu_k * dC_k/dp_m = 0
```

### Term 1: dW/dp_m

For buyers on market m: `d(surplus * q)/dp = -q_i` (higher price reduces buyer surplus)
For sellers on market m: `d(surplus * q)/dp = +q_i` (higher price increases seller surplus)

So: `dW/dp_m = -sum_{buyers} q_i + sum_{sellers} q_i = -(D_m - S_m)` at the fill quantities.

### Term 2: lambda_m * d(D-S)/dp_m

The excess demand response to price. As price increases, demand decreases and supply increases (more sellers willing to trade, fewer buyers). This is the standard tatonnement force.

### Term 3: -sum_k mu_k * dC_k/dp_m

For BuyYes orders on market m: `d(p*q)/dp = q_i`, so price increase costs more capital.
For SellYes orders on market m: `d((1-p)*q)/dp = -q_i`, so price increase uses less capital.

Combining:

```
dC_k/dp_m = sum_{BuyYes in MM(k), market m} q_i - sum_{SellYes in MM(k), market m} q_i
```

## What Tatonnement Computes

Standard tatonnement uses the gradient step:

```
p_m <- p_m + lr * excess_demand_m(p)
```

This converges to `D_m(p) = S_m(p)`, which is the market-clearing condition. In KKT terms, tatonnement finds a stationary point of:

```
dL/dp_m = lambda_m * d(D-S)/dp_m = 0   (for mu_k = 0)
```

This is correct when **there are no active MM budget constraints** (mu_k = 0 for all k).

## What Tatonnement Misses

When an MM budget is binding (mu_k > 0), the KKT optimality condition requires:

```
dW/dp_m + lambda_m * d(D-S)/dp_m = sum_k mu_k * dC_k/dp_m
```

The right-hand side is **non-zero** when MM orders are active. The optimal prices are **shifted** relative to pure market-clearing prices.

### Direction of the Price Shift

At the optimum with an active MM budget (mu_k > 0):

- **Markets where MMs buy YES**: `dC_k/dp_m > 0`, so the constraint pushes p_m **down** (reduces capital per unit, allows more fills within budget)
- **Markets where MMs sell YES**: `dC_k/dp_m < 0`, so the constraint pushes p_m **up** (same effect: reduces 1-p capital)

The price correction is:

```
Delta_p_m ~ -mu_k * (sum_{MM buys YES on m} q_i - sum_{MM sells YES on m} q_i)
```

This bias is proportional to:
1. The shadow price mu_k of the binding budget constraint
2. The net MM position on each market

## Practical Implications

### Why MILP Beats Tatonnement by 4-5x

Tatonnement finds prices where D(p) = S(p), then extracts fills at those prices. When MM budgets are tight:

1. **Wrong prices**: Tatonnement prices don't account for the capital constraint bias. The optimal prices should be shifted to pack more welfare into the limited budget.

2. **Wrong fill selection**: Given wrong prices, fill extraction picks suboptimal orders. A slightly lower price on a market where MMs buy YES would allow the MM to fill more orders total.

3. **Cascading effect**: MM fills interact with user fills through position balance. Wrong MM prices cascade into wrong user fill decisions.

### Proposed Fix: Budget-Aware Price Correction

After tatonnement converges to market-clearing prices p*, apply a correction:

```
p_corrected_m = p*_m - lr_mu * sum_k mu_k * net_mm_demand_m(k)
```

Where mu_k can be estimated via bisection on the budget constraint:
1. Start with mu_k = 0
2. Compute fills at corrected prices
3. If capital_used > B_k, increase mu_k
4. Converge when capital_used ≈ B_k

This turns an O(n log n) tatonnement into O(n log n * log(1/epsilon)) with the outer bisection, but captures the key MM budget interaction that tatonnement currently misses.

### Alternative: Lagrangian Relaxation

Instead of post-hoc correction, embed the MM budget dual directly in the tatonnement iteration:

```
p_m <- p_m + lr * (excess_demand_m - sum_k mu_k * d_capital_k/dp_m)
mu_k <- max(0, mu_k + lr_mu * (C_k(p,q) - B_k))
```

This is a primal-dual method that jointly optimizes prices and budget duals. It converges to the KKT point directly, but requires tuning two learning rates and may oscillate.

## Summary

| Property | Tatonnement | KKT Optimal |
|----------|-------------|-------------|
| Objective | D(p) = S(p) | dL/dp = 0 |
| MM budget | Post-hoc enforcement | Built into prices |
| Price bias | None (market-clearing) | Shifted by mu * dC/dp |
| Welfare | Local optimum | Global (with MILP) |

The gap between tatonnement and MILP is fundamentally due to the missing mu_k terms in the price gradient. The larger the MM budget utilization, the larger this gap will be.
