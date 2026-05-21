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

The solver has four mechanisms, each addressing different constraints:

| # | Mechanism | Constraint Handled | Approach |
|---|-----------|-------------------|----------|
| 1 | Unified binary clearing | P_YES + P_NO = $1 (per market) | Construction |
| 2 | Negrisk arbitrage feedback | Σ P_YES = $1 (per group) | Heuristic iteration |
| 3 | Dual decomposition | Σ P_YES = $1 + MM budgets | Lagrangian relaxation |
| 4 | Multi-market matching | Bundle/spread fill atomicity | Complement + leg decomposition |

Mechanisms 2 and 3 are alternative approaches to the cross-market problem.
Mechanism 4 runs as a partial solver in any pipeline configuration.
The system has several pipeline configurations:

- `Pipeline::with_negrisk()` — Mechanisms 1 + 2 + 4, with MM allocation
- `Pipeline::with_dual_decomposition()` — Mechanisms 1 + 3 + 4

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

## Historical Mechanism 2: Negrisk Arbitrage Feedback

The canonical sequencer path does not emit synthetic orders or synthetic fills.
Minting/burning is represented by the protocol MINT account during settlement.
This section documents the older feedback mechanism as solver history only.

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
    1. Price Discovery (LocalSolver) — historical internal feedback orders only
    2. Negrisk detection — creates historical feedback orders for next iteration
    3. MM Allocation — activates orders within budget
    check convergence (welfare delta < threshold)
```

#### Step 4: Historical internal fill filtering

Historical feedback orders participated in clearing (consume liquidity,
influence prices), but their fills were filtered out of final output. The
current canonical path is stricter: `MatchingResult`, blocks, and witnesses
contain only real participant fills, and minting/burning is accounted for by
MINT settlement.

### What it guarantees

- Price sum error decreases over iterations (empirically converges to ~2%)
- All canonical output fills are real participant fills
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
| Multi-market orders | Via MultiMarketSolver (partial solver) | Via MultiMarketSolver (partial solver) |
| Maturity | Battle-tested on sims | Newer, passes all tests |

### Limitations

**Multi-market orders** (bundles, spreads) are handled by the MultiMarketSolver
(Mechanism 4), which runs as a partial solver after dual decomposition.
However, it operates independently — dual variable information is not used to
guide multi-market matching, and multi-market fills do not feed back into the
dual iteration.

**Duality gap**: the subgradient method finds the dual optimum, but primal
recovery (mapping dual variables to fills) may have a gap. The final pass
validates and enforces hard constraints.

**Mixed orders**: `is_seller()` checks if any payoff is negative. For complex
derivatives (spreads with both positive and negative payoffs), the buyer/seller
classification may be ambiguous. This affects welfare computation and bid
shading direction.

---

## Mechanism 4: Multi-Market Matching

**File:** `specialized/multi_market.rs` (MultiMarketSolver)

### What it does

Matches multi-market orders (bundles, spreads) that span multiple binary markets.
These orders cannot be filled by per-market price discovery alone because their
payoff depends on the joint outcome of several markets. The solver uses two
complementary strategies, run in sequence.

### Strategy 1: Complement Matching

Orders with identical markets and negated payoffs cancel perfectly:

- `bundle_yes({A,B})` payoff `[+1, 0, 0, 0]` + `bundle_sell({A,B})` payoff `[-1, 0, 0, 0]`
- `spread(A,B)` payoff `[0, -1, +1, 0]` + `spread_sell(A,B)` payoff `[0, +1, -1, 0]`

No decomposition needed. Standard bid/ask matching within each group.

#### How it works

1. **Group** multi-market orders by `PayoffKey = (sorted_markets, |payoffs|)`.
2. **Split** each group into two sides by the sign of the first non-zero payoff.
3. **Match** greedily: compute the valid fill_price range for each order
   (buyer: `F ∈ [0, limit]`, seller: `F ∈ [limit, ∞)`), intersect the ranges,
   and fill at the midpoint if the intersection is non-empty.
4. Fill quantity = `min(a.max_fill, b.max_fill)`, respecting AON constraints.

### Strategy 2: Leg Decomposition

Decomposes a multi-market order into per-market legs using marginal payoff
averaging (the same decomposition used by settlement), then matches each leg
against single-market counterparties.

#### The decomposition

For each market in the order, average the payoff over states where that market
has each outcome:

```
For market M, outcome o:
  leg_shares = Σ(payoff[s] for s where M has outcome o) / count(such states)
```

**Examples:**
- `bundle_yes({A,B})` `[+1, 0, 0, 0]` → +1/2 A-YES, +1/2 B-YES per unit
- `spread(A,B)` `[0, -1, +1, 0]` → +1/2 A-YES, -1/2 A-NO, -1/2 B-YES, +1/2 B-NO

#### How it works

1. **Build a counterparty pool** from unfilled single-market orders (excluding
   MM-constrained orders), indexed by `(market, outcome)`. Sellers sorted by
   price ascending, buyers by price descending.
2. **For each unfilled multi-market order** (sorted by welfare potential desc):
   a. Compute legs via `compute_legs()`.
   b. For each positive leg (need to buy): consume cheapest sellers from pool.
   c. For each negative leg (need to sell): consume best-priced buyers from pool.
   d. Compute `total_cost = Σ(buy costs) - Σ(sell revenues)`.
   e. **Buyer constraint**: `total_cost ≤ limit × qty`.
      **Seller constraint**: `-total_cost ≥ limit × qty` (sufficient revenue).
   f. If feasible: emit fill for the multi-market order and consume pool liquidity.
3. **Aggregate counterparty fills**: accumulate per-order_id to avoid duplicate
   Fill records. Emit one Fill per counterparty with weighted average price.
   Verify each counterparty fill respects AON and limit price constraints.

#### Fill price calculation

- **Buyer orders**: `fill_price = total_cost / fill_qty`
- **Seller orders**: `fill_price = (-total_cost) / fill_qty` (revenue per unit)

This ensures welfare is always non-negative for both sides.

### What it guarantees

- **Per-market netting**: for each (market, outcome), every long position has a
  corresponding short position. The platform has zero residual exposure.
- **Limit respect**: all fills satisfy buyer/seller limit constraints.
- **AON respect**: all-or-none constraints checked on both multi-market orders
  and counterparties.
- **No duplicate fills**: each counterparty order emits at most one Fill.
- **Non-negative welfare**: every fill has welfare ≥ 0.

### What it does NOT guarantee

- **Welfare optimality**: greedy matching by welfare potential. Orders are
  processed sequentially; a different ordering could yield higher total welfare.
- **Liquidity efficiency**: once a counterparty is consumed by one multi-market
  order, it's unavailable for others that might have produced more welfare.
- **Partial fills for multi-market orders**: currently fills at max_fill or not
  at all. Partial fill support would require more complex leg quantity scaling.

### Integration

MultiMarketSolver implements `PartialSolver` and runs as a pipeline step in both
`with_negrisk()` and `with_dual_decomposition()` configurations. Its fills are
combined with other partial solver fills via the MWIS combiner on a conflict graph.

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

### Historical Negrisk Pipeline

```
Pipeline::with_negrisk().solve(&problem)
  |
  +-- Iteration 1..5:
  |   +-- LocalSolver (unified binary clearing per market)
  |   +-- NegriskSolver (historical: detect price sum deviations)
  |   +-- MmAllocator (greedy budget-constrained activation)
  |   +-- MultiMarketSolver (complement match + leg decomposition)
  |   +-- Check convergence (welfare delta)
  |
  +-- Return only real participant fills
  +-- Combine partial solutions (MWIS on conflict graph)
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
  +-- MultiMarketSolver (complement match + leg decomposition)
  +-- Combine partial solutions (MWIS on conflict graph)
  +-- Return fills, prices, welfare
```

---

## How We Know It Works

### Test Coverage

**Unit tests (78 in matching-solver):**
- Per-market clearing correctness (supply/demand crossing, price consistency)
- Seller-aware welfare computation and price satisfaction
- MM budget allocation with tight budgets, overlapping MMs, varied welfare
- Dual decomposition convergence, shading math, residual computation
- Multi-market solver: complement matching (bundles, spreads), leg decomposition,
  AON, counterparty stacking, welfare positivity, per-market netting
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

### Key Metrics (Extreme scenario, dual solver)

| Metric | Value | Interpretation |
|--------|-------|----------------|
| Fill rate | 72K/103K (70%) | Healthy — limited by liquidity scarcity |
| MM fill rate | 278/1200 (23%) | Budget-constrained, as designed |
| Bundle fill rate | 8.2K/25K (33%) | Multi-market matching active |
| Bundle welfare | $151K | Positive, via complement + leg decomposition |
| Total welfare | $1.01M | Positive, non-negative per fill |
| Verification | VALID | All ZK invariants pass |

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

2. **Multi-market matching is greedy**. The MultiMarketSolver handles bundles
   and spreads via complement matching and leg decomposition, but the greedy
   ordering (by welfare potential) may miss globally better combinations.
   Integration with the dual decomposition's price signals could improve
   multi-market fill rates and welfare.

3. **Mixed-payoff order ambiguity**. `is_seller()` checks for any negative
   payoff. Complex derivatives (spreads, straddles) may be misclassified,
   leading to incorrect welfare calculation or bid shading direction.

4. **Historical Negrisk feedback was non-atomic**. Individual legs could fill
   while others did not. This is one reason the canonical path now keeps
   minting/burning in MINT settlement rather than synthetic order output.

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

**6. Deeper bundle integration into dual decomposition.**
The MultiMarketSolver currently runs independently of the dual iteration.
Adding coupling constraints for multi-market orders (one Lagrange multiplier
per bundle's atomicity constraint) would let dual decomposition guide
multi-market pricing. This increases the dual space significantly but produces
more coherent bundle pricing.

**7. Partial fills for multi-market orders.**
The MultiMarketSolver currently fills at max_fill or not at all. Supporting
partial fills would require scaling leg quantities proportionally and
re-checking cost constraints, but would increase fill rates on
liquidity-constrained legs.

### What Would NOT Help

- **Direct price normalization** (scaling prices to sum=$1): breaks limit price
  guarantees. Rejected for good reason.
- **More negrisk iterations** (beyond 5): diminishing returns. The fundamental
  issue is that fair-share is a heuristic, not that we iterate too few times.
- **MILP for the full problem**: intractable for 100K+ orders. The
  decomposition approach is the right architecture; the question is how good
  the decomposition is.
