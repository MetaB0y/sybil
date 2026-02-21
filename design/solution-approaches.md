# Solution Approaches for Welfare-Maximizing Matching

## Context

See [problem-statement.md](problem-statement.md) for the full problem definition. The key insight: **without MM budgets, the problem is an LP**. The bilinear MM budget constraint ($p \times q \leq B$) is the only source of non-convexity. This document surveys principled approaches for handling the full problem.

---

## 0. Current State and Why It Struggles

The current solver pipeline decomposes the LP into phases (LocalSolver → NegriskSolver → DualMaster → MmAllocator → enforce_ucp). This is an elaborate heuristic decomposition of an LP, patched with post-hoc corrections. It achieves only 20-25% of the welfare-optimal solution on realistic scenarios (4-5x gap vs MILP).

The gap comes from:
- **46%**: No group minting (the LP handles this trivially via `gmint_g` variables)
- **34%**: DualMaster's bid shading inflates limits beyond original values → fills rejected
- **20%**: Fixed-point search stuck in local price equilibria

All three sources vanish if we solve the LP directly.

---

## 1. Direct LP Solve (Baseline — No MM Budgets)

### What

Formulate the market clearing as an LP and solve with an off-the-shelf solver.

### Formulation

```
max  Σ_buyers L_i·q_i - Σ_sellers L_j·q_j - $1·Σ_m mint_m - $1·Σ_g gmint_g

s.t.
  ∀m,o: Σ_buy(m,o) q_i ≤ Σ_sell(m,o) q_j + mint_m + 1[o=0]·gmint_{g(m)}
  ∀i:   0 ≤ q_i ≤ Q̄_i
        mint_m, gmint_g ≥ 0
```

### Output

- Primal: fill quantities `q_i`, minting quantities
- Dual: clearing prices `p_m` (from position balance), price normalization (from mint stationarity)

### Complexity

$O(N + M + G)$ variables and constraints. Modern LP solvers (HiGHS, CLP): <1ms for 10K orders.

### What This Gives Us

- **Optimal welfare** for the LP relaxation (no MM budgets)
- **Correct prices** from dual variables (UCP, normalization, group consistency — all free)
- **Group minting** handled naturally (`gmint_g` variables)
- **No need for**: LocalSolver, NegriskSolver, DualMaster, enforce_ucp, group_minting.rs

### Limitations

Does not handle MM budget constraints. MM orders are treated as regular orders with unlimited capital.

### Implementation

LP solvers with Rust bindings: [HiGHS](https://github.com/rust-or/highs) (BSD, fast), [good_lp](https://github.com/rust-or/good_lp) (multi-backend abstraction — already used by MILP solver).

### Integer Quantities

The LP gives continuous $q_i$. For integer fills, either:
- Round (at most 1 unit of welfare loss per order — negligible)
- Add integrality constraints → MILP (but the LP relaxation is tight in practice because order quantities are large relative to rounding error)

---

## 2. Iterative LP (Simple Fixed-Point for MM Budgets)

### Idea

Solve the LP with all MM orders included. Check budgets at the dual prices. If a budget is violated, remove the least efficient MM orders and re-solve.

### Algorithm

```
active_mm = {all MM orders}

for iter in 0..MAX_ITERS:
    # Solve LP with current active MM set
    (q*, p*) = solve_clearing_lp(non_mm_orders ∪ active_mm)

    # Check MM budgets at dual prices p*
    violations = {}
    for each MM k:
        usage_k = Σ_{i ∈ orders(k) ∩ active_mm} capital(p*_{m(i)}, q*_i)
        if usage_k > B_k:
            violations[k] = usage_k - B_k

    if no violations:
        break  # Budget-feasible optimum found

    # Remove least-efficient MM orders from violating MMs
    for each violating MM k:
        Sort orders in k by welfare/capital ratio at (q*, p*)
        Remove lowest-ratio orders until usage ≤ B_k

    Update active_mm

return (q*, p*)
```

### Properties

- **Each iteration solves the LP exactly** — prices are correct for the given order set
- **Budget feasibility guaranteed** at termination (we remove orders until budgets are satisfied)
- **Not globally optimal** — removing orders changes prices, which could make previously removed orders feasible again
- **Convergence**: guaranteed in at most |MM_orders| iterations (worst case, all MM orders removed)

### Complexity

At most $K$ LP solves where $K$ = number of MM orders. Each LP solve: <1ms. Total: <10ms typically (converges in 2-4 iterations).

### Pros

- Extremely simple to implement
- Each iteration is exact (LP + deterministic budget check)
- Fast convergence in practice

### Cons

- Not globally optimal (greedy removal is irreversible in this formulation)
- May under-utilize MM budgets if removal cascades

### Required: Bidirectional Iteration

After convergence, re-add previously removed MM orders (highest welfare/capital first). If the LP still satisfies budgets with the re-added order, keep it. Repeat until stable.

This is **not optional** for production. Market makers are the primary liquidity providers. Greedy one-directional removal can drain liquidity from low-volume markets (where MM supply is the only supply). Bidirectional iteration ensures maximum MM participation within budget constraints, which matters both for welfare and for keeping MMs happy with the exchange.

---

## 3. Benders Decomposition

### Idea

Separate the problem into:
- **Master**: choose which MM orders to include (binary decisions)
- **Subproblem**: given the active MM set, solve the market clearing LP (get welfare and prices)

The master uses information from the subproblem (Benders cuts) to guide the search for the optimal MM activation.

### Formulation

**Master problem** (integer program):
```
max   θ                           # welfare estimate
s.t.  θ ≤ W_t + π_t · (z - z_t)  # Benders optimality cuts (from each iteration t)
      Σ_{i∈k} z_i·capital_lb_i ≤ B_k · (1 + slack)  # Budget relaxation
      z_i ∈ {0, 1}                # MM order activation
```

**Subproblem** (LP, for given $z^*$):
```
Solve clearing LP with orders = {non-MM orders} ∪ {MM order i : z*_i = 1}
→ Returns: welfare W, dual prices p, dual multipliers π
```

**Budget check**:
```
For each MM k: compute capital at (q*, p*)
If violated → generate feasibility cut for master
If feasible → generate optimality cut for master
```

### Algorithm

```
Initialize: z = all-ones (all MMs active)
UB = +∞, LB = -∞

for iter in 0..MAX_ITERS:
    # Solve subproblem
    (q*, p*, W*) = solve_clearing_lp(z)

    # Check budgets
    for each MM k:
        usage_k = Σ capital(p*, q*_i) for active orders in k

    if all budgets satisfied:
        UB = min(UB, W*)
        if UB - LB < tolerance: break  # Optimal!
        Add optimality cut to master

    else:
        # Generate feasibility cut: linearize budget around (q*, p*)
        # Cut says: "this z combination needs at least X more slack"
        Add feasibility cut to master

    # Solve master
    z = solve_master()
    LB = master_objective

return best feasible (q*, p*)
```

### Non-Convexity Caveat

**Important**: The budget constraint $p \cdot q \leq B$ is bilinear (non-convex). Standard Benders optimality proofs rely on the subproblem's feasible region being convex. Because the budget maps the dual price $p$ (which changes with $z$) against the primal $q$, the function $f(z) =$ "budget usage when LP is solved with MM set $z$" is **not convex in $z$**.

This means simple tangent-hyperplane cuts from first-order Taylor linearization may slice off valid parts of the search space, and **global optimality is not guaranteed by naive Benders**.

**Fix — Spatial Bounding**: To recover rigorous bounds, the master problem must use **McCormick envelopes** (or similar convex relaxations) for the bilinear budget terms rather than simple tangent cuts. This makes the master's feasible region a valid outer approximation:
```
For each bilinear term c = p·q in the budget:
  c ≥ p_L·q + p·q_L - p_L·q_L    (McCormick lower)
  c ≥ p_U·q + p·q_U - p_U·q_U
  c ≤ p_U·q + p·q_L - p_U·q_L    (McCormick upper)
  c ≤ p_L·q + p·q_U - p_L·q_U
```
With tight price bounds $[p_L, p_U]$ (which the LP dual provides), McCormick gives a reasonably tight LP relaxation. Iterative bound tightening (solving LP → narrowing price bounds → tightening McCormick → re-solving) converges to the global optimum.

Without McCormick, Benders still produces good feasible solutions and useful bounds — it just lacks the formal guarantee.

### Properties

- **Master problem is small**: only |MM_orders| binary variables (typically 50-5000)
- **Each subproblem is fast**: one LP solve (<1ms)
- **Provides welfare bounds**: UB (master relaxation) and LB (best feasible) bracket the optimum
- **With McCormick**: converges to global optimum; without: converges to good local optimum

### Complexity

Typically 10-50 Benders iterations. Each iteration: 1 LP solve + 1 small MIP solve. Total: 50-200ms (the master MIP is the bottleneck, but with only |MM_orders| binary variables, it's fast).

### Pros

- With McCormick envelopes, provides rigorous global optimality
- Provides optimality certificate (gap bound)
- Clean separation of LP and combinatorial parts
- Well-studied theory (Generalized Benders Decomposition handles non-convex subproblems)

### Cons

- More complex to implement than iterative LP
- Master MIP solver dependency (though same solver as LP — HiGHS handles MIP)
- May be slow if many Benders iterations needed (pathological cases with many interacting MMs)
- McCormick envelopes require iterative bound tightening for tight relaxations

---

## 4. Frank-Wolfe / Conditional Gradient

### Idea

Treat the bilinear budget constraint as a smooth nonlinear constraint and use the Frank-Wolfe algorithm: at each step, linearize the budget constraint at the current solution, solve the resulting LP, and move toward the LP solution.

### Algorithm

```
# Initial solve: LP without budget constraints
(q⁰, p⁰) = solve_clearing_lp(all orders)

for t in 0..MAX_ITERS:
    # Linearize budget constraint at current (qᵗ, pᵗ):
    # capital ≈ pᵗ·q + p·qᵗ - pᵗ·qᵗ  (first-order Taylor of p·q)
    # Since p is dual, express as modified LP:

    # Solve LP with linearized budget as extra constraints
    (q̃, p̃) = solve_clearing_lp_with_budget_cuts(qᵗ, pᵗ)

    # Frank-Wolfe step: move toward LP solution
    γ = 2 / (t + 2)  # or line search
    qᵗ⁺¹ = (1 - γ)·qᵗ + γ·q̃

    # Recompute prices from LP dual at new q
    # (or re-solve LP with q fixed to get p)

    if converged: break
```

### Handling Dual Variables in the Cut

The budget constraint is $\sum_i p_{m(i)} \cdot q_i \leq B$, where $p$ is a dual variable. To linearize:

At current solution $(q^t, p^t)$:
$$\sum_i \left[ p^t_{m(i)} \cdot q_i + q^t_i \cdot p_{m(i)} - p^t_{m(i)} \cdot q^t_i \right] \leq B_k$$

Since $p_{m(i)}$ is the dual of the balance constraint for market $m(i)$, this can be reformulated as a constraint on the primal LP variables (it adjusts the objective coefficients for MM orders based on the budget shadow price).

Practically: add the linearized budget as a primal constraint. The LP solver handles it naturally.

### Properties

- **Convergence**: $O(1/t)$ to local optimum (standard Frank-Wolfe rate)
- **Each iteration**: one LP solve
- **No integer variables** — purely continuous
- **Gap certificate**: Frank-Wolfe gap provides upper bound on suboptimality

### Complexity

20-50 iterations × LP solve time. Total: 20-50ms.

### Pros

- Simple, elegant algorithm
- Each step is just an LP solve — same infrastructure as baseline
- Good convergence rate for this type of problem
- No MIP solver needed

### Cons

- Converges to local optimum, not global (bilinear constraint is non-convex)
- Linearization of dual variables requires care (dual may change discontinuously)
- Frank-Wolfe can be slow in the "tail" (zigzagging near optimum)

### Enhancement: Away-Step Frank-Wolfe

Standard Frank-Wolfe can zigzag. The away-step variant maintains an active set and can also move *away* from bad vertices, giving linear convergence rate.

---

## 5. Entropy Smoothing / Deterministic Annealing

### Idea

Replace hard fill decisions with smooth sigmoid functions. The welfare landscape becomes differentiable, enabling gradient-based optimization of the full problem (including MM budgets) as a single unified optimization.

### Smooth Fill Function

Instead of $q_i \in \{0, \bar{Q}_i\}$ (hard fill), use:

$$q_i(p, \varepsilon) = \bar{Q}_i \cdot \sigma\!\left(\frac{\text{surplus}_i(p)}{\varepsilon}\right)$$

where $\sigma(x) = 1/(1+e^{-x})$ is the sigmoid and $\varepsilon > 0$ is the temperature.

- **High $\varepsilon$**: fills are smooth (near 0.5 for all orders) — landscape has one broad peak
- **Low $\varepsilon$**: fills are sharp (near 0 or 1) — recovers the hard fill decisions
- **$\varepsilon \to 0$**: exact LP solution

### Full Lagrangian

$$\mathcal{L}(p, \lambda, \mu) = \underbrace{\sum_i \text{surplus}_i(p) \cdot q_i(p, \varepsilon)}_{\text{smoothed welfare}} + \underbrace{\sum_m \lambda_m \cdot \text{balance}_m(q(p))}_{\text{position balance}} + \underbrace{\sum_k \mu_k \cdot (B_k - \text{capital}_k(p, q(p)))}_{\text{MM budget}}$$

### Algorithm

```
Initialize: p = uniform prices (equal within groups), λ = 0, μ = 0
ε = ε_start (e.g., 0.1 × $1)

for cooling_step in 0..NUM_COOLING:
    for inner_step in 0..INNER_ITERS:
        # Compute smoothed fills at current prices
        q = sigmoid_fills(p, ε)

        # Gradient of L w.r.t. prices
        ∇p = d_welfare/dp + λ · d_balance/dp + μ · d_capital/dp

        # Price update (projected gradient step)
        p ← p - α · ∇p
        p ← project_onto_simplex(p)  # ensure Σp ≤ $1 per group

        # Dual updates (subgradient ascent)
        λ ← λ + β · balance_violation
        μ ← max(0, μ + β · budget_violation)

    ε ← ε × cooling_factor (e.g., 0.5)

# Final rounding
q_final = round_fills(q(p, ε_final))
verify(q_final, p)
```

### Connection to LMSR

Hanson's Logarithmic Market Scoring Rule (the foundation of prediction market AMMs like Polymarket) is **exactly** the single-market-maker version of entropy-smoothed welfare maximization. Our multi-order batch auction with smoothing is the generalization to multiple participants.

This connection gives strong theoretical grounding and makes the approach publishable as a contribution to both mechanism design and optimization.

### Lagrangian Decomposition

At fixed dual variables $(\lambda, \mu)$, the smoothed problem **decomposes by market group**:
- Each group subproblem is a $K$-dimensional optimization on the price simplex
- Number of dual variables = $|M|$ (balance) + $|K|$ (MM budgets)
- Not exponential — polynomial in problem size

### Properties

- **Single unified optimization** — no pipeline, no phases, no decomposition hacks
- **Handles all constraints** via Lagrangian (prices, groups, budgets)
- **Differentiable** everywhere (smoothing removes kinks)
- **Deterministic annealing** — theoretically principled (tracks the global optimum as temperature decreases)
- **Quality certificate**: Lagrangian dual provides upper bound on optimal welfare

### Complexity

Outer loop (cooling): 10-20 steps. Inner loop: 50-100 gradient steps. Each step: $O(N)$ (compute fills and gradients). Total: $O(N \cdot 1000) \approx$ 10ms for 10K orders.

### Pros

- Most elegant — single optimization, no decomposition
- Handles everything jointly (prices, fills, minting, budgets)
- Theoretically grounded (LMSR connection, deterministic annealing theory)
- GPU-parallelizable (gradient computation is embarrassingly parallel)
- Most novel for publication

### Cons

- Local optima (annealing doesn't guarantee global convergence)
- Rounding at the end may lose welfare (need careful repair)
- Integer quantities need handling (continuous relaxation + rounding)
- Step size tuning and cooling schedule require experimentation
- Interaction between smoothing and Lagrangian relaxation needs validation

### Operational Concerns for Production

While entropy smoothing is the most elegant approach, it introduces risks for a financial exchange:

- **Reproducibility**: Gradient-based methods with temperature schedules can produce different results across hardware due to floating-point accumulation order. A financial exchange must guarantee deterministic clearing. Mitigation: use fixed-point arithmetic throughout, or restrict to integer-safe operations.
- **Explainability**: Exchanges must explain clearing outcomes. "The LP dual price exceeded your limit" is clear. "The subgradient ascent of the smoothed Lagrangian prioritized a different price manifold" is not. The final rounded solution should be explainable in LP terms (prices and surpluses), even if the solver used smoothing internally.
- **Auditability**: For regulatory and ZK-proof purposes, the verifier must check the final (rounded) solution, not the smoothed intermediate. The rounding step must preserve all invariants.

These concerns make entropy smoothing better suited as a **research/publication** approach or as an **internal solver** whose output is verified and explained via LP duality, rather than as the user-facing clearing mechanism.

### Open Questions

1. **Cooling schedule**: geometric decay? adaptive based on gradient magnitude?
2. **Step size**: fixed, diminishing 1/√t, or Adam-style adaptive?
3. **Rounding**: randomized rounding with constraint preservation?
4. **Position balance with soft fills**: Lagrangian or projection onto balanced manifold?

---

## 6. ADMM (Alternating Direction Method of Multipliers)

### Idea

Split the problem into an LP part and a budget-feasibility part, linked by consensus constraints. ADMM alternates between solving each part and enforcing agreement.

### Splitting

Introduce copies: $q^{LP}$ (LP variables) and $q^{MM}$ (budget variables), with consensus $q^{LP} = q^{MM}$.

**LP step**: solve market clearing LP with augmented Lagrangian penalty for disagreement with $q^{MM}$.

**Budget step**: project $q^{LP}$ (plus dual) onto the budget-feasible set. For each MM, this is a knapsack projection at current prices.

**Dual step**: update multipliers for consensus violation.

### Algorithm

```
Initialize: q = LP solution (no budgets), u = 0 (dual), ρ = penalty parameter

for iter in 0..MAX_ITERS:
    # LP step: solve augmented LP
    q_lp = argmin { -welfare(q) + (ρ/2)||q - q_mm + u||² : LP constraints }

    # Budget step: project onto budget feasibility
    q_mm = budget_project(q_lp + u, prices, budgets)

    # Dual update
    u = u + q_lp - q_mm

    if primal_residual < tol and dual_residual < tol: break
```

### Properties

- **Convergence**: guaranteed for convex problems; for bilinear (non-convex), converges to stationary point
- **Each step is tractable**: LP step is an LP with quadratic penalty (QP); budget step is a projection (knapsack per MM)
- **ρ tuning**: adaptive ρ (increase when primal residual >> dual, decrease otherwise)

### Complexity

20-50 iterations. Each iteration: one QP solve + one budget projection. Total: 50-100ms.

### Pros

- Well-studied convergence theory
- Natural splitting for this problem (LP vs. budgets)
- Handles non-convexity reasonably well in practice

### Cons

- QP step (LP + quadratic penalty) is slightly more expensive than pure LP
- Convergence may be slow if ρ is poorly tuned
- Not globally optimal for non-convex problems
- More complex than Frank-Wolfe or iterative LP

---

## 7. ML-Guided Warm Start

### Idea

Train a neural network to predict good clearing prices given an order book. Use the prediction as a warm start for any of the above methods.

### Architecture

```
Input:  Per-market features (# orders, price distribution, total demand/supply,
        MM orders present, group membership)
Output: Predicted clearing prices per market

Training data: MILP solutions on generated scenarios (the MILP solver already exists)
```

### How It Helps

- **Warm start for LP**: Simplex method from a good basis converges in fewer pivots
- **Warm start for Benders**: Good initial $z$ (MM activation) reduces iterations
- **Warm start for annealing**: Start prices near optimal → less cooling needed
- **Warm start for MILP**: Feasible solution as MIP start → tighter bounds, faster branching

### Implementation

Train offline (hours). Inference: <1ms (small MLP). Improvement: potentially 2-5x speedup on exact methods by reducing iterations.

### Pros

- Orthogonal to all other approaches (pure speedup, no accuracy loss)
- Training data is free (generate scenarios, solve with MILP)
- Inference is trivial (<1ms)

### Cons

- Doesn't improve solution quality directly
- Requires training infrastructure
- Model may not generalize to novel order distributions
- Not publishable on its own (supplementary technique)

---

## 8. Comparison

| Approach | Optimality | Speed (est.) | Complexity | MM Budgets | Publishable |
|----------|-----------|-------------|------------|------------|-------------|
| Direct LP (no MM) | Optimal (LP) | <1ms | Low | No | Foundation |
| Iterative LP | Local | <10ms | Low | Yes (heuristic) | Low |
| Benders + McCormick | Global* | 50-200ms | Medium-High | Yes (rigorous) | Medium |
| Frank-Wolfe | Local | 20-50ms | Low | Yes (linearized) | Medium |
| Entropy Smoothing | Local† | ~10ms | Medium | Yes (Lagrangian) | **High** |
| ADMM | Stationary | 50-100ms | Medium | Yes (split) | Medium |
| ML Warm Start | N/A (speedup) | <1ms | Low | N/A | Low |

*Global with McCormick bound tightening; without McCormick, good local optimum with bounds.
†With good annealing schedule, tracks the global optimum in practice but no formal guarantee.

---

## 9. Recommended Path

### Phase 1: LP Foundation (Low risk, high impact)

**Replace the entire current pipeline with a direct LP solve.**

- Implement the LP formulation from the problem statement
- Use HiGHS (already has Rust bindings, BSD licensed, state-of-the-art)
- This alone should close the 46% gap (group minting) and 20% gap (price exploration)
- MM orders participate as regular orders (unlimited budget) — optimistic upper bound on welfare
- Compare welfare against current pipeline and MILP

Expected result: LP welfare ≥ MILP welfare (LP is a relaxation of MILP, which has integrality constraints). But LP fills may violate MM budgets.

### Phase 2: MM Budget Handling (Medium risk)

**Start with iterative LP** (simplest). Benchmark against MILP.

If iterative LP gets within 5% of MILP on MM budget utilization → ship it.

If not → implement **Frank-Wolfe** (next simplest, better convergence properties).

If Frank-Wolfe still has significant gap → implement **Benders** (globally optimal, more complex).

### Phase 3: Publication (if desired)

**Entropy smoothing** is the most publishable approach:
- Novel application of deterministic annealing to FBA
- Connection to LMSR gives theoretical depth
- Handles all constraints in a single unified optimization
- Story: "We show that welfare-maximizing FBA with budget constraints admits an LP formulation. We propose entropy-smoothed gradient descent to handle the bilinear budget coupling, achieving near-optimal welfare in milliseconds."

Can be implemented in parallel with Phase 2 (different people, or as an experimental alternative).

### Throughout: MILP as Benchmark

Keep the existing MILP solver as ground truth. All new approaches are measured against it. The MILP solver's `group_mint_g` variable and bilinear budget handling are the gold standard.

---

## 10. What About Bundles?

Bundles (multi-market orders, ~15% of volume) add cross-market coupling beyond MM budgets. Two options:

### Option A: Marginal Decomposition in the LP

Decompose each bundle into per-market legs using marginal payoffs. Include legs in the per-market LP as fractional demand/supply. This is an approximation but handles most cases correctly.

### Option B: Arrow-Debreu State Constraints

Add per-state balance constraints for each compound state that appears in a bundle's payoff vector. Exact, but increases LP size by $O(\sum_i 2^{K_i})$ where $K_i$ is the number of markets bundle $i$ spans. For typical bundles (2-3 markets), this is manageable.

### Option C: Post-Processing

Solve the LP for single-market orders, then match bundles against residual liquidity (as the current MultiMarketSolver does). Simple and works well when bundles are a small fraction.

**Recommendation**: Start with Option A or C, upgrade to B if bundle welfare matters.

---

## References

- Boyd et al. (2011). *Distributed Optimization and Statistical Learning via ADMM.* Foundations and Trends in ML.
- Benders (1962). *Partitioning procedures for solving mixed-variables programming problems.* Numerische Mathematik.
- Budish, Cramton, Shim (2015). *The High-Frequency Trading Arms Race.* QJE.
- Frank, Wolfe (1956). *An algorithm for quadratic programming.* Naval Research Logistics.
- Hanson (2003). *Combinatorial Information Market Design.* Information Systems Frontiers.
- Nesterov (2005). *Smooth Minimization of Non-Smooth Functions.* Mathematical Programming.
- Rose (1998). *Deterministic Annealing for Clustering, Compression, Classification, Regression, and Related Optimization Problems.* Proceedings of the IEEE.
