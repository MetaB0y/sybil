# Smoothed Batch Auction: Theory and Algorithm

## 1. Problem Statement

We run **Frequent Batch Auctions (FBA)** for prediction markets. Each batch:

- **Markets**: Binary (YES/NO). Organized into **groups** of mutually exclusive outcomes (exactly one resolves YES). Some markets are standalone (group of 1).
- **Orders**: Each order has a **payoff vector** over market states, a **limit price**, and a **max quantity**. Single-market orders touch one market. **Bundle orders** span 2-5 markets across groups.
- **Market Makers (MMs)**: Submit many orders but with a shared **budget constraint** — total capital used across all their fills must not exceed their budget. Capital depends on fill price × quantity (bilinear).
- **Goal**: Find **clearing prices** and **fill quantities** that **maximize welfare** (total consumer surplus), subject to:
  - **Universal Clearing Price (UCP)**: One price per market per batch.
  - **Complementary slackness**: Orders fill only if profitable at clearing prices.
  - **Position balance**: For each market, total YES fills = total NO fills (conservation).
  - **Group consistency**: YES prices within a group sum to $1 (no-arbitrage).
  - **MM budget**: Each MM's total capital usage ≤ budget.

## 2. Key Insight: Groups = Arb = Bundles (Unified View)

A group constraint "prices of A, B, C sum to $1" is equivalent to a permanent infinite-quantity arb order that buys 1 share of each outcome for $1. This is the same structure as any bundle order whose payoff cancels to a constant.

**All market coupling comes from orders (or structural constraints) whose payoff vectors span multiple markets.** The solver should not distinguish between "group coupling" and "bundle coupling" — they're the same thing at different strengths.

The **coupling graph**: markets are nodes, multi-market orders are (hyper)edges. Groups create cliques of strongly-coupled markets. Bundles create weaker edges. Markets with no shared orders are independent.

## 3. The Pricing Kernel

By the fundamental theorem of asset pricing, prices are arbitrage-free iff there exists a **probability distribution π over states** such that every asset's price equals its expected payoff under π.

For our grouped binary markets:
- States = product of group outcomes. For groups of sizes K₁, K₂, ..., K_G: |S| = Π Kᵍ states.
- Market k's YES price = marginal probability that outcome k wins.
- Group sum = 1 follows automatically (marginals sum to 1).
- Bundle price = expected payoff under the joint distribution π.

**Without bundles**: π factors into independent per-group distributions. Each group has Kᵍ - 1 free parameters (a small simplex). Groups don't interact.

**With bundles**: The joint distribution matters. A bundle spanning groups g₁, g₂ depends on π_{g₁,g₂}, not just the marginals. The joint distribution space is exponentially large (Π Kᵍ), but we never represent it explicitly — we work with marginals + dual variables for bundle constraints.

## 4. The Welfare Landscape

For fixed prices, optimal fills are determined by complementary slackness (fill if profitable, don't if not). Welfare as a function of prices is **piecewise-linear**: within each "regime" (set of willing orders), welfare is linear. Regimes change at order limit prices.

Properties:
- **Not convex, not concave** — it's a jagged landscape of plateaus and cliffs.
- **For 1 market**: O(n) regimes on a line. LocalSolver finds optimum in O(n log n).
- **For K-market group**: O(n^{K-1}) regimes on a (K-1)-simplex. Expensive to enumerate for K > 3.
- **Gradient is zero almost everywhere** (flat plateaus), undefined at kinks.

This is why direct gradient descent fails on the raw welfare function.

## 5. Entropy Smoothing

Replace hard complementary slackness with a smooth approximation.

### The idea

Add an entropy regularization term to the welfare objective:

```
W_ε(prices) = max over fills:
    Σᵢ (limitᵢ - priceᵢ) · qᵢ  +  ε · Σᵢ H(qᵢ / Qᵢ)
    subject to: position balance, 0 ≤ qᵢ ≤ Qᵢ
```

where H(x) = x·ln(x) + (1-x)·ln(1-x) is the binary entropy and ε > 0 is the **temperature**.

### What it does

1. **Removes kinks**: The optimal fills become a smooth function of prices (by the envelope theorem). The welfare landscape becomes differentiable.
2. **Partial fills near the price**: Orders slightly in-the-money partially fill (e.g., 80%). Orders slightly out-of-the-money also partially fill (e.g., 20%). The transition is sigmoid-shaped, controlled by ε.
3. **As ε → 0**: Recovers exact complementary slackness (hard fill/no-fill).

### Connection to LMSR

Hanson's Logarithmic Market Scoring Rule (LMSR) is exactly the entropy-regularized single-market-maker version of this problem. Our multi-order batch auction with entropy smoothing is the generalization to multiple agents. This connects to the prediction market theory literature.

### Gradient computation

With smoothing, the fill of order i at price p is approximately:

```
qᵢ(p) ≈ Qᵢ · sigmoid((limitᵢ - p) / ε)
```

(Exact form depends on position balance constraints, but this is the intuition.)

The welfare gradient with respect to price p on market m:

```
∂W_ε/∂p_m = Σᵢ on market m: [∂qᵢ/∂p_m · surplusᵢ + qᵢ · (-1)]
```

One pass through orders on market m. O(n) per market.

## 6. The Algorithm: Gradient Descent + Annealing

### Core loop

```
Initialize prices: p_m = 1/K for each market in group of size K
                   p_m = 0.5 for standalone markets
Set ε = ε_start (e.g., 0.1 * NANOS_PER_DOLLAR)

Repeat until ε < ε_min:
    # Inner loop: gradient descent at current temperature
    Repeat until convergence:
        For each group g (parallelizable):
            Compute gradient of smoothed welfare w.r.t. group prices
            Take gradient step
            Project onto simplex (prices ≥ 0, sum = 1)

        For each bundle dual variable λ_b:
            Compute bundle constraint violation
            λ_b += step · violation

        For each MM dual variable μ_j:
            Compute budget constraint violation
            μ_j += step · violation

    # Cool down
    ε *= cooling_factor (e.g., 0.5)

# Final: extract exact solution
Round fills to integers
Enforce hard complementary slackness
Enforce position balance (trim imbalances)
Run verifier
```

### Why it works

- **High ε**: Smooth landscape, one broad peak. Gradient descent finds it quickly.
- **Gradually lower ε**: Peak sharpens, but we track it. Each cooling step only needs a few gradient iterations (warm start from previous ε).
- **Final rounding**: At ε ≈ 0, fills are nearly 0 or Q. Rounding to exact integers introduces tiny error.

### This IS simulated annealing + gradient descent

| Simulated Annealing     | Our Algorithm                                        |
|-------------------------|------------------------------------------------------|
| High temperature        | High ε: orders partially fill at unfavorable prices  |
| Low temperature         | Low ε: only clearly profitable orders fill           |
| Anneal (cool gradually) | Decrease ε gradually                                 |
| Random walk exploration | Gradient descent (directed, efficient)               |

We get SA's ability to escape local optima (via temperature) with GD's efficiency (via gradient information). This is known as **deterministic annealing** or **graduated optimization**.

## 7. Lagrangian Decomposition for Bundles and MMs

### The coupling problem

Single-market orders only depend on their market's price (marginal). Bundle orders depend on the joint distribution across groups. MMs couple all markets they participate in.

Representing the full joint distribution is exponential in the number of groups. We avoid this using **Lagrangian relaxation**.

### Setup

For each bundle order b, relax its pricing constraint (that its price must be consistent with some joint distribution) with a multiplier λ_b:

```
L(λ, μ) = max over {group prices, fills}:
    Σ (single-market welfare)
    + Σ_b λ_b · (bundle surplus at current prices)
    - Σ_j μ_j · (MM capital usage - budget)
    subject to: per-group simplex constraints, position balance
```

### Decomposition

At fixed (λ, μ), this **decomposes by group**:

```
Group g: max welfare_g(fills, prices_g)
         + Σ_{bundles touching g} λ_b · (group g's contribution to bundle b)
         - Σ_{MMs with orders in g} μ_j · (capital of MM j's orders in g)
```

Each group subproblem is small: optimization on a K-dimensional simplex.

### Dual update

```
λ_b += step · bundle_surplus_b      (if bundle wants to fill but can't)
μ_j += step · (capital_j - budget_j) (if MM budget exceeded)
```

### Complexity

- **Per iteration**: O(|groups|) independent subproblems + O(|bundles| + |MMs|) dual updates.
- **Number of dual variables**: |bundles| + |MMs|. NOT exponential.
- **Convergence**: O(1/√T) duality gap with subgradient. O(1/T²) with smoothing + Nesterov acceleration.
- **Duality gap = quality certificate**: Upper bound on optimal welfare. If gap is small, we're provably near-optimal.

### Dense bundles (adversarial case)

If an adversary submits bundles connecting all groups:
- Algorithm still polynomial per iteration.
- More bundles = more dual variables = slower convergence.
- **Defenses**: (a) prune low-welfare bundles, (b) time budget with duality gap reporting, (c) hypergraph partitioning to decompose into tractable clusters.

## 8. Physical Analogy

Think of each order as a **spring** with rest position at its limit price.

- **Clearing price** = a ball connected to all springs on its market. It settles where forces balance (supply = demand).
- **Group constraint** = rigid rod connecting balls (prices sum to $1).
- **Bundle dual variable** = elastic rod connecting balls across groups (tries to enforce consistency, can stretch).
- **Entropy smoothing** = making springs slightly soft near rest position (instead of snapping from stretched to slack).
- **Temperature annealing** = gradually stiffening the springs.
- **Gradient descent** = simulating the physics. The system relaxes to equilibrium (welfare maximum).

This is mathematically precise: the gradient of smoothed welfare IS the net force. Gradient descent IS force-directed relaxation.

## 9. Comparison to Current Pipeline

### Current (NegriskSolver / DualMaster)

```
LocalSolver (per-market) → NegriskSolver (arb orders, iterate) → MmAllocator (knapsack)
→ DualMaster (Lagrangian per-market) → reclear_groups (binary search hack)
→ enforce_ucp (reprice, trim) → apply_minting → enforce_mm_budget
```

Seven phases. Each creates artifacts the next must clean up. Group consistency is a post-hoc fix. Bundles handled by injection of synthetic orders. MM budget enforced by removing fills after the fact.

### New (Smoothed Gradient)

```
One loop:
    gradient step on group prices (smoothed welfare)
    dual update for bundles
    dual update for MMs
    decrease temperature
Then: round + verify
```

One loop, three update types. Group consistency is built-in (simplex projection). Bundles handled by dual variables. MM budget handled by dual variables. No post-hoc fixes needed.

## 10. What We Keep From the Current Codebase

- **matching-engine**: All types (Order, Fill, Market, MarketGroup, Problem, MmConstraint). Untouched.
- **matching-solver/local_solver.rs**: Useful as the 1D subroutine for coordinate-descent within groups. May also serve as initialization (run LocalSolver first, then refine with gradient).
- **matching-solver/verifier.rs**: Essential for validating solutions. Untouched.
- **matching-scenarios**: Test scenario generation. Untouched.
- **matching-sim**: CLI and comparison framework. Minor changes to wire new solver.
- **MILP solver**: Keep as gold standard benchmark.

## 11. Open Questions

1. **Position balance in the smoothed problem**: With soft fills, position balance (YES qty = NO qty) becomes a soft constraint too. How to handle? Options: (a) Lagrangian relaxation (add dual variable per market), (b) enforce exactly by parameterizing fills on the balanced manifold, (c) project onto balanced fills after each gradient step.

2. **Step size selection**: Standard options — fixed, diminishing (1/√t), or adaptive (Adam, AdaGrad). Need to experiment.

3. **Convergence criteria**: When to stop inner loop? When to stop annealing? Duality gap provides a principled answer but need to implement.

4. **Integer rounding**: Final fills must be integer quantities (in nanos). The smoothed solution gives fractional fills. Rounding can violate constraints. Need careful rounding + repair.

5. **Interaction between smoothing and Lagrangian**: Both are relaxations. Do they compose well? Theory says yes (smooth dual functions are easier to optimize), but need to verify empirically.

6. **How to handle the per-group subproblem**: Pure gradient descent on the simplex, or coordinate descent using LocalSolver as 1D oracle? The latter reuses existing code and may be faster (LocalSolver is exact in 1D).

## References

- Hanson, R. (2003). Combinatorial Information Market Design. Information Systems Frontiers.
- Chen, Y. & Pennock, D. (2007). A Utility Framework for Bounded-Loss Market Makers.
- Budish, E., Cramton, P., Shim, J. (2015). The High-Frequency Trading Arms Race.
- Nesterov, Y. (2005). Smooth Minimization of Non-Smooth Functions.
- Fisher, M.L. (1981). The Lagrangian Relaxation Method for Solving Integer Programming Problems.
- Boyd, S. et al. (2011). Distributed Optimization and Statistical Learning via ADMM.
