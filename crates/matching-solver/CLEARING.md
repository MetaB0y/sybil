# Market Clearing and Price Consistency

How the solver turns a batch of orders into fills with prices that make economic
sense.

## The Problem

We run a prediction market where each event (e.g. "2024 Election") has multiple
mutually exclusive outcomes (Trump, Harris, Other). Each outcome is a separate
binary market. These binary markets are linked by a `MarketGroup` that declares
them mutually exclusive. There are no "native" multi-outcome markets.

A binary market has two states: YES (state 0) and NO (state 1).

**Coupling constraints** make this harder than independent per-market clearing:

1. **Price consistency:** across a MarketGroup, YES prices must sum to $1.
   If Trump=0.45, Harris=0.40, Other=0.15, sum=1.00. Deviation = free money.
2. **MM budget limits:** market makers post orders across many markets, but their
   total capital exposure is capped. Fills must respect this joint budget.

The solver has three mechanisms, each addressing different constraints:

| # | Mechanism | Constraint Handled | Approach |
|---|-----------|-------------------|----------|
| 1 | Unified binary clearing | P_YES + P_NO = $1 (per market) | Construction |
| 2 | Negrisk arbitrage feedback | Σ P_YES = $1 (per group) | Heuristic iteration |
| 3 | Dual decomposition | Σ P_YES = $1 + MM budgets | Lagrangian relaxation |

Mechanisms 2 and 3 are alternative approaches to the cross-market problem.
The system has several pipeline configurations:

- `Pipeline::with_negrisk()` — Mechanisms 1 + 2, with MM allocation
- `Pipeline::with_dual_decomposition()` — Mechanisms 1 + 3

---

## Mechanism 1: Unified Binary Clearing

**File:** `local_solver.rs`, method `solve_binary_market_unified`

### What it does

Each binary market has one price. A YES share at price P implies a NO share at
$1 - P. Unified clearing finds this single price by merging all order flow —
YES buyers, NO buyers, sellers — into one supply/demand model.

### The key identity

**Buying NO at price Q = selling YES at price ($1 - Q).**

This means every NO buyer adds YES supply, and every NO seller adds YES demand.
A single supply-demand crossing determines P_YES, and P_NO = $1 - P_YES follows
automatically.

### How it works

1. **Classify orders** by payoff vector:
   - `payoffs[0] > 0` → YES buyer
   - `payoffs[1] > 0` → NO buyer
   - `payoffs[0] < 0` → YES seller
   - `payoffs[1] < 0` → NO seller

2. **Build unified YES supply and demand:**
   - YES demand = YES buyers (at their limit prices)
     + NO sellers (converted: sell NO at P → buy YES at $1-P)
   - YES supply = liquidity book asks
     + YES sellers (at their limit prices)
     + NO buyers (converted: buy NO at P → sell YES at $1-P)

3. **Find clearing price P_YES** via supply-demand crossing.
   P_NO = $1 - P_YES. One market, one price, exact.

4. **Generate fills:**
   - YES buyers filled at P_YES (buyers pay ≤ limit)
   - NO buyers filled at P_NO = $1 - P_YES
   - Sellers filled at clearing price (sellers receive ≥ limit)

### What it guarantees

- P_YES + P_NO = $1 exactly, by construction
- All fills respect limit prices (buyer-aware and seller-aware)
- Per-market welfare is non-negative (each fill has non-negative surplus)

### What it does NOT do

It does not coordinate prices **across** markets. Three binary markets in an
election group can independently clear at YES prices 0.50 + 0.40 + 0.40 = 1.30.
Each is internally consistent (YES + NO = $1), but the group violates sum=$1.
That's what Mechanisms 2 and 3 address.

---

## Mechanism 2: Negrisk Arbitrage Feedback

**File:** `negrisk.rs` (NegriskSolver) + `pipeline.rs` (feedback loop)

### What it does

When YES prices across a MarketGroup don't sum to $1, the NegriskSolver creates
synthetic orders that enter the next iteration's price discovery, pushing
clearing prices toward the correct sum through actual market forces.

### How it works

#### Step 1: Detect the opportunity

For each MarketGroup, sum the YES prices:

- **Negrisk** (sum < $1): buy YES on all → guaranteed $1 payout for < $1 cost.
- **Posrisk** (sum > $1): buy NO on all → guaranteed $(N-1) payout for < $(N-1)
  cost (since sum_NO = N×$1 - sum_YES < N-1).

#### Step 2: Create orders with fair-share limits

For each market, create one single-market order:

- **Negrisk**: buy YES (`payoffs[0] = 1`)
- **Posrisk**: buy NO (`payoffs[1] = 1`)

The limit price is the **fair-share value** — the current price scaled
proportionally so the group sums to exactly $1:

```
fair_yes_i = current_yes_i × $1 / sum_yes
```

**Why fair-share:** current-price limits create zero pressure (orders placed at
existing clearing price → no demand/supply shift). Fair-share limits undercut
existing prices, creating actual market force.

**Is this optimal?** No. Fair-share is a heuristic that approximates the dual
price signal of the sum=$1 constraint. It pushes prices in the right direction
but does not guarantee global welfare optimality.

**Is this atomic?** No. The arb legs are independent single-market orders. Partial
execution is possible. The orders are a coordination signal, not a real hedge.

#### Step 3: Iterate

The pipeline runs a fixed-point loop (max 5 iterations):

```
for each iteration:
    1. Price Discovery (LocalSolver) — includes arb orders from previous iteration
    2. Negrisk detection — creates new arb orders for next iteration
    3. MM Allocation — activates orders within budget
    check convergence (welfare delta < threshold)
```

#### Step 4: Arb fill filtering

Arb orders participate in clearing (consume liquidity, influence prices) but
their fills are filtered out of the final output. Only real participant fills
appear in the MatchingResult. The last iteration's arb orders never clear — a
small welfare loss accepted for soundness.

### What it guarantees

- Price sum error decreases over iterations (empirically converges to ~2%)
- All output fills are real (no synthetic fills in result)
- Individual market clearing remains exact (P_YES + P_NO = $1)

### What it does NOT guarantee

- **Convergence**: runs for a fixed number of iterations, no formal proof
- **Atomicity**: arb legs can partially execute
- **Welfare optimality**: heuristic decomposition, not joint optimization
- **MM budget integration**: budgets handled separately by MmAllocator (post-hoc)

---

## Mechanism 3: Dual Decomposition

**File:** `dual_master.rs` + `pipeline.rs` (via `Pipeline::with_dual_decomposition()`)

### What it does

Handles both coupling constraints (price consistency AND MM budgets) through
**Lagrangian dual decomposition**. Instead of synthetic orders, it adjusts order
limit prices using dual variables (Lagrange multipliers) so that per-market
clearing naturally tends toward the coupled constraints.

### Architecture

```
Pipeline::with_dual_decomposition().solve(&problem)
  |
  +-- DualMaster::solve()
  |   |
  |   +-- Initialize: λ=0 (price consistency), μ=0 (pacing)
  |   |
  |   +-- Iteration loop (max 20):
  |   |   +-- 1. shade_orders(): adjust limits using λ, μ
  |   |   +-- 2. LocalSolver: solve per-market with shaded orders
  |   |   +-- 3. compute_primal_residuals(): measure constraint violations
  |   |   +-- 4. update_duals(): subgradient step on λ, μ
  |   |   +-- 5. check_convergence(): primal feasibility + dual stability
  |   |
  |   +-- Final pass: re-solve with converged shading, validate
  |   |   against original limits and MM budgets
  |   |
  |   +-- Return fills, prices, convergence diagnostics
  |
  +-- Return PipelineResult
```

### Bid Shading

The Lagrangian relaxes coupling constraints into penalty terms that modify each
order's effective limit price.

**Price Consistency (λ per MarketGroup):**

For group g with constraint Σ P_YES_i = $1:

| Order type | Effective limit |
|------------|----------------|
| YES buyer | `limit - λ × $1` |
| NO buyer | `limit + λ × $1` |
| YES seller | `limit - λ × $1` |
| NO seller | `limit + λ × $1` |

When λ > 0 (posrisk, sum > $1): YES buyers bid less, YES supply increases → YES
prices drop. When λ < 0 (negrisk): the reverse.

**Pacing (μ per MM):**

For MM k with constraint Σ capital ≤ B_k:

| Order type | Paced limit |
|------------|-------------|
| Buy (YES/NO) | `limit / (1 + μ)` |
| Sell (YES/NO) | `(limit + μ × $1) / (1 + μ)` |

When μ > 0 (over budget): MM bids less aggressively, gets fewer fills, uses less
capital. Both adjustments compose: pacing first, then price consistency.

### Subgradient Updates

After each iteration, measure constraint violations (primal residuals):

- Price residual: `r_λ = (Σ P_YES - $1) / $1` per group
- Budget residual: `r_μ = (capital_used - budget) / budget` per MM

Update dual variables:

- `λ += α_t × r_λ` (unconstrained — can be positive or negative)
- `μ = max(0, μ + α_t × r_μ)` (projected — must be non-negative)

Step size: `α_t = α_0 / √t` (diminishing, standard for subgradient methods).
Default: α_0 = 0.5.

### Final Pass

After convergence, re-solve with the converged shading to get the final fills.
Then validate:

1. **Limit check**: each fill must satisfy `order.is_satisfied_at_price(fill_price)`.
   Fills that violate original limits (possible due to shading) are dropped.
2. **MM budget enforcement**: greedily include MM fills sorted by welfare
   descending, skipping any that would exceed the budget.

### Convergence Properties

The subgradient method guarantees convergence to within ε of the dual optimum
for convex problems with diminishing step sizes. Per-market subproblems are
piecewise-linear (LP), so the Lagrangian dual is convex.

In practice, convergence is checked on two criteria:
- **Primal feasibility**: all residuals < 2% tolerance
- **Dual stability**: multiplier changes < 0.1% tolerance

### Comparison to Mechanism 2

| Property | Negrisk (Mech 2) | Dual Decomposition (Mech 3) |
|----------|-----------------|---------------------------|
| Price consistency | Fair-share heuristic | Subgradient (principled) |
| MM budgets | Post-hoc greedy | Bid shading (in-clearing) |
| Convergence | No formal guarantee | Subgradient guarantee (convex dual) |
| Implementation | Synthetic orders | Dual variable adjustment |
| Multi-market orders | Not handled | Not handled |
| Maturity | Battle-tested on sims | Newer, passes all tests |

### Limitations

**Multi-market orders** (bundles, spreads) are not handled by either pipeline.
They are matched only if they happen to clear within a single market's
price discovery. A scalable multi-market clearing mechanism is needed.

**Duality gap**: the subgradient method finds the dual optimum, but primal
recovery (mapping dual variables to fills) may have a gap. The final pass
validates and enforces hard constraints.

**Mixed orders**: `is_seller()` checks if any payoff is negative. For complex
derivatives (spreads with both positive and negative payoffs), the buyer/seller
classification may be ambiguous. This affects welfare computation and bid
shading direction.

---

## MM Budget Allocation

**File:** `mm_allocator.rs`

Used by the negrisk pipeline (Mechanism 2). Dual decomposition (Mechanism 3)
handles MM budgets directly through pacing multipliers.

### How it works

1. Receive fills from price discovery (which orders matched and at what price)
2. For unfilled MM orders, estimate fills at clearing prices with max quantity
3. Compute welfare and capital cost for each MM order:
   - Welfare: `order.welfare_contribution(fill_price, fill_qty)`
   - Capital: `side.capital_needed(price, qty)` — BuyYes costs `price × qty`,
     SellYes costs `(1 - price) × qty`
4. Sort by welfare/capital ratio (descending)
5. Greedily activate until budget exhausted

### Budget tracking across iterations

In the fixed-point pipeline, MM fills accumulate across iterations. Each
iteration's allocator sees a reduced budget (original minus capital already
committed). The final reported allocation shows cumulative totals with the
original budget.

---

## Order Representation

**File:** `matching-engine/src/order.rs`

Orders use a payoff vector representation that supports arbitrary derivatives:

```rust
struct Order {
    payoffs: [i8; 32],     // per-state payoffs (positive=long, negative=short)
    limit_price: Nanos,    // max willingness to pay (buyer) / min to receive (seller)
    min_fill, max_fill: Qty,
    // ...
}
```

### Buyer vs. Seller Detection

```rust
fn is_seller(&self) -> bool {
    self.payoffs[..num_states].iter().any(|&p| p < 0)
}
```

An order is a seller if it has any negative payoff (short exposure). This drives:

- **welfare_contribution**: `(limit - price) × qty` for buyers,
  `(price - limit) × qty` for sellers
- **is_satisfied_at_price**: buyer wants `price ≤ limit`, seller wants
  `price ≥ limit`
- **Verifier checks**: PriceExceedsLimit is buyer/seller-aware

**Limitation**: for complex derivatives with mixed payoffs (e.g., a spread that
is long one outcome and short another), `is_seller()` returns true even though
the order is not purely a seller. Welfare calculation in this case may not
perfectly reflect economic surplus.

---

## Result Verification

**File:** `verifier.rs`

The verifier checks every invariant that a ZK circuit would enforce:

| Check | What it validates |
|-------|-------------------|
| OrderNotFound | Fill references an order in the problem |
| QuantityExceedsMax | fill_qty ≤ order.max_fill |
| QuantityBelowMin | fill_qty ≥ order.min_fill (or fill_qty = 0) |
| PriceExceedsLimit | Buyer: fill_price ≤ limit. Seller: fill_price ≥ limit |
| DuplicateFill | Each order filled at most once |
| NegativeWelfare | Each fill has welfare ≥ 0 |
| WelfareMismatch | Computed total = reported total (± tolerance) |
| MmBudgetExceeded | MM capital_used ≤ max_capital |
| ZeroQuantityFill | No zero-qty fills (strict mode only) |

Two modes:
- **Lenient** (default): allows zero fills, 1000 nanos welfare tolerance
- **Strict** (ZK): no zero fills, zero tolerance

---

## Architecture: What happens per batch

### Negrisk Pipeline

```
Pipeline::with_negrisk().solve(&problem)
  |
  +-- Iteration 1..5:
  |   +-- LocalSolver (unified binary clearing per market)
  |   +-- NegriskSolver (detect price sum deviations, create arb orders)
  |   +-- MmAllocator (greedy budget-constrained activation)
  |   +-- Check convergence (welfare delta)
  |
  +-- Filter arb fills from output
  +-- Report cumulative MM allocation stats
  +-- Return fills, prices, welfare
```

### Dual Decomposition Pipeline

```
Pipeline::with_dual_decomposition().solve(&problem)
  |
  +-- DualMaster (λ/μ iteration → shaded clearing → convergence)
  |   +-- Final pass: validate limits + enforce MM budgets
  |
  +-- Return fills, prices, welfare
```

---

## How We Know It Works

### Test Coverage

**Unit tests (70 in matching-solver):**
- Per-market clearing correctness (supply/demand crossing, price consistency)
- Seller-aware welfare computation and price satisfaction
- MM budget allocation with tight budgets, overlapping MMs, varied welfare
- Dual decomposition convergence, shading math, residual computation
- Verifier catches all violation types
- Property-based tests: budget constraint ALWAYS respected (proptest)

**Integration tests (17):**
- `tests/validation.rs`: price ranges, MM budgets, fill limits on realistic problems
- `tests/dual_decomposition.rs`: price sum convergence, MM budget respected,
  non-negative welfare, limit satisfaction, dual vs. negrisk comparison,
  two-outcome convergence, no-coupling-constraint fast convergence
- `tests/welfare_analysis.rs`: stage-by-stage welfare audit

**Simulation (`matching-sim`):**
- Presets: small (10 markets), medium, large, extreme (200 markets, 100K orders, 10 MMs)
- Extreme scenario: 100K+ orders, 200 markets, 10 MMs with $50K-$200K budgets
- All presets pass ZK verification (status: VALID)

### Key Metrics (Extreme scenario)

| Metric | Value | Interpretation |
|--------|-------|----------------|
| Fill rate | 64K/101K (63%) | Healthy — limited by liquidity scarcity |
| MM fill rate | 291/1200 (24%) | Budget-constrained, as designed |
| MM utilization | 99-100% per MM | Budgets fully used |
| Welfare | $800K | Positive, non-negative per fill |
| Verification | VALID | All ZK invariants pass |
| Negrisk convergence | 26 groups corrected | Sum→$1 via arb feedback |

### What We Don't Yet Measure

- **Welfare gap vs. joint LP optimal**: we don't solve the joint LP to compare.
  This is the most important missing benchmark. (Note: a naive Arrow-Debreu LP
  has O(2^k) solvency constraints for k markets, making it impractical when
  bundle orders chain many markets into one connected component.)
- **Price sum error distribution**: we know convergence happens but don't
  systematically track residual error across scenarios.
- **Dual decomposition vs. negrisk welfare comparison**: both run on the same
  problems but we haven't built regression benchmarks.

---

## Known Limitations and Potential Improvements

### Current Limitations

1. **No global welfare optimality guarantee**. Each market is cleared
   independently. A joint LP would find a better or equal solution. Both
   negrisk and dual decomposition are decomposition heuristics.

2. **Multi-market orders are second-class**. Bundles and spreads are not
   matched by any solver. They only fill if they happen to clear within
   per-market price discovery. A scalable multi-market clearing mechanism
   is needed.

3. **Mixed-payoff order ambiguity**. `is_seller()` checks for any negative
   payoff. Complex derivatives (spreads, straddles) may be misclassified,
   leading to incorrect welfare calculation or bid shading direction.

4. **Negrisk arb orders are non-atomic**. Individual legs can fill while
   others don't. The synthetic arbitrageur has exposed risk. This is
   internal to the solver (doesn't affect real participants) but makes arb
   welfare accounting inaccurate.

5. **No adaptive step size in dual decomposition**. Fixed α_0/√t schedule.
   If convergence stalls, there's no recovery mechanism (e.g., Polyak step
   or restart).

### Potential Improvements (Ordered by Impact)

**1. Welfare benchmarking against joint LP.**
Solve the joint LP (even just for small problems or random samples) to measure
the welfare gap. This tells us how much the decomposition costs in practice.
Note: a naive Arrow-Debreu LP has O(2^k) solvency constraints and doesn't
scale when bundle orders chain markets into large components. Constraint
generation (lazy solvency checking) or Benders decomposition may be needed.

**2. Tighten dual decomposition tolerances.**
The 2% primal tolerance means price sums can be 0.98 or 1.02. For prediction
markets where prices ARE probabilities, this is loose. Reducing to 0.5% would
require more iterations but produce more consistent prices. Investigate
Polyak step size (uses function value bound) for faster convergence.

**3. Integrate MM budgets into negrisk pipeline.**
Currently negrisk uses post-hoc MM allocation (MmAllocator). The dual
decomposition's pacing approach (bid shading for budget constraints) could
be backported to the negrisk pipeline for better budget-aware clearing.

**4. Mixed-payoff order handling.**
Replace `is_seller()` with a more nuanced classification for multi-outcome
orders. For spreads (long one outcome, short another), welfare should consider
both legs. This matters for correctness of welfare computation and verifier.

**5. Adaptive step size for dual decomposition.**
Implement Polyak step size: `α_t = (f* - f_t) / ||g_t||²` where f* is a
target (e.g., best known dual value). This adapts to problem difficulty and
avoids the slow tail convergence of 1/√t.

**6. Bundle integration into dual decomposition.**
Add coupling constraints for multi-market orders (one Lagrange multiplier per
bundle's atomicity constraint). This increases the dual space significantly
but produces more coherent bundle pricing. Only worth it if bundle volume is
high.

### What Would NOT Help

- **Direct price normalization** (scaling prices to sum=$1): breaks limit price
  guarantees. Rejected for good reason.
- **More negrisk iterations** (beyond 5): diminishing returns. The fundamental
  issue is that fair-share is a heuristic, not that we iterate too few times.
- **MILP for the full problem**: intractable for 100K+ orders. The
  decomposition approach is the right architecture; the question is how good
  the decomposition is.
