# Market Clearing and Price Consistency

How the solver turns a batch of orders into fills with prices that make economic
sense.

## The Problem

We run a prediction market where each event (e.g. "2024 Election") has multiple
mutually exclusive outcomes (Trump, Harris, Other). Each outcome is a separate
binary market. These binary markets are linked by a `MarketGroup` that declares
them mutually exclusive. This is the only multi-outcome representation — there
are no "native" multi-outcome markets.

A binary market has two states: YES (state 0) and NO (state 1).

The fundamental constraint: across a MarketGroup, YES prices must sum to $1.
If Trump=0.45, Harris=0.40, Other=0.15, then sum=1.00. If they don't sum to $1,
there is free money on the table (arbitrage).

The solver has two mechanisms:

1. **Unified binary clearing** — within each binary market, a single clearing
   price P is found. P_NO = $1 - P automatically. One market, one price.
2. **Negrisk arbitrage feedback** — across a MarketGroup, synthetic arbitrage
   orders push YES prices toward sum=$1 through market forces.

---

## Mechanism 1: Unified Binary Clearing

**File:** `local_solver.rs`, method `solve_binary_market_unified`

### What it does

Each binary market has one price. A YES share at price P implies a NO share at
$1 - P. Unified clearing finds this single price by merging all order flow —
YES buyers, NO buyers, sellers — into one supply/demand model.

### Why it exists (the bug it fixes)

The old code (`solve_per_outcome`) ran two independent auctions per market:

```rust
for outcome in 0..num_outcomes {
    let (price, fills, ..) = self.solve_outcome(market_id, outcome, ..);
    prices[outcome] = price;
}
```

Outcome 0 (YES) had its own supply-demand crossing. Outcome 1 (NO) had its own.
Two separate auctions produced two unrelated prices — P_YES + P_NO could be
anything. In a real exchange this can't happen: a YES bid IS a NO offer. But the
batch solver had broken this linkage.

This made negrisk feedback impossible: NO-buy arb orders (for posrisk) were
invisible to the YES auction, so they had zero effect on YES prices.

### How it works

The key identity: **buying NO at price Q = selling YES at price ($1 - Q)**.

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
   - YES buyers filled at P_YES
   - NO buyers filled at P_NO = $1 - P_YES
   - Sellers similarly

### Example

Trump market:
- 100 YES buyers at limit $0.60 (100 shares each)
- 50 NO buyers at limit $0.70 (100 shares each) — from negrisk arb

NO buyers convert to YES supply at $1 - $0.70 = $0.30:
- YES demand: 10,000 shares at $0.60
- YES supply: 5,000 shares at $0.30

Crossing: P_YES = $0.30, matched = 5,000 shares.
P_NO = $0.70. The NO buyers pushed the YES price down to $0.30.

---

## Mechanism 2: Negrisk Arbitrage Feedback

**File:** `negrisk.rs` (NegriskSolver) + `pipeline.rs` (feedback loop)

### What it does

When YES prices across a MarketGroup don't sum to $1, the NegriskSolver creates
synthetic orders that enter the next iteration's price discovery, pushing
clearing prices toward the correct sum through actual market forces.

### Why it exists

Unified clearing ensures P_YES + P_NO = $1 **within** each binary market. But
the three separate markets in an election group can still have YES prices
0.50 + 0.40 + 0.40 = 1.30 (too high). Each market is individually consistent
(YES + NO = $1), but the group is not (YES prices don't sum to $1).

Direct price normalization (scaling prices) was rejected: it changes prices
without market justification and can make clearing prices violate order limits.

### How it works

#### Step 1: Detect the opportunity (`negrisk.rs`)

For each MarketGroup, sum the YES prices:

- **Negrisk** (sum < $1): buy YES on all → guaranteed $1 payout for < $1 cost.
- **Posrisk** (sum > $1): buy NO on all → guaranteed $(N-1) payout for < $(N-1)
  cost (since sum_NO = N×$1 - sum_YES < N-1).
- **No arb** (sum = $1): nothing to do.

#### Step 2: Create orders with fair-share limits

For each market, create one single-market order:

- **Negrisk**: buy YES (`payoffs[0] = 1`)
- **Posrisk**: buy NO (`payoffs[1] = 1`)

The limit price is set to the **fair-share value** — the current price scaled
proportionally so the group would sum to exactly $1:

```
fair_yes_i = current_yes_i * $1 / sum_yes
```

For posrisk (buy NO):
```
limit_i = $1 - fair_yes_i = $1 - current_yes_i / sum_yes
```

**Why fair-share instead of current-price limits:** if arb orders use the
current price as limit (e.g., Harris NO limit = $0.01 when Harris YES = $0.99),
they convert to YES supply at $0.99 — the existing clearing price. Zero price
pressure. With fair-share (Harris NO limit = $0.60), they convert to YES supply
at $0.40, undercutting the $0.99 clearing and pushing the price down.

**Is this equivalent to solving a dual LP?** No. The correct approach would be
a joint LP maximizing welfare across all markets subject to sum=$1. Fair-share
pricing is a heuristic that approximates the dual price signal. It creates
pressure in the right direction but does not guarantee global welfare optimality.
See [Formal Properties](#formal-properties) below.

**Is this atomic?** No. The arb orders are individual single-market orders, not
an atomic bundle. Trump NO can fill while Harris NO doesn't. The synthetic
"arbitrageur" can lose money on individual legs. This is a known limitation —
the orders are a coordination signal, not a real hedged position.

#### Step 3: Iterate (`pipeline.rs`)

The pipeline runs a fixed-point loop (max 5 iterations):

```
for each iteration:
    1. Price Discovery (LocalSolver) — includes arb orders from previous iteration
    2. Negrisk detection — creates new arb orders for next iteration
    3. MM Allocation — activates orders within budget
    4. Partial solvers (ArbitrageDetector) — bundle matching
    check convergence (welfare delta < threshold)
```

Arb orders from iteration N enter iteration N+1's LocalSolver. In unified
clearing, posrisk NO-buy arb orders become YES supply, pushing YES prices down.
Negrisk YES-buy arb orders add YES demand, pushing YES prices up. Over
iterations, prices converge toward sum=$1.

#### Step 4: Arb fill filtering

Arb orders participate in clearing (they consume liquidity and influence prices)
but their fills are **filtered out of the final output**. Only real participant
fills appear in the `MatchingResult`. This is correct because:

- Arb orders have no real account behind them — settlement would skip them
- Including them would inflate welfare, volume, and fill count metrics
- Their purpose is price coordination, not actual trading

The last iteration's arb orders never enter a subsequent price discovery pass.
These orders are simply dropped — no synthetic fills are injected. This accepts
a small welfare loss in exchange for soundness.

### Concrete example: Election scenario

Initial state (batch 0, first price discovery):
- Trump YES: 0.50, Harris YES: 0.99, Other YES: 0.99
- Sum = 2.48 (posrisk, profit = $1.48/share)

NegriskSolver creates posrisk arb orders (buy NO):
- Trump: NO limit = $1 - 0.50/2.48 = $0.80 → YES supply at $0.20
- Harris: NO limit = $1 - 0.99/2.48 = $0.60 → YES supply at $0.40
- Other: NO limit = $1 - 0.99/2.48 = $0.60 → YES supply at $0.40

Next iteration, unified clearing sees this new YES supply. Harris YES supply at
$0.40 undercuts the previous $0.99 clearing, pulling Harris YES down.

After 5 iterations, final prices:
- Trump: 0.47, Harris: 0.41, Other: 0.13
- Sum = 1.02 (from 2.48)

---

## Architecture: What happens per batch

```
Sequencer receives orders for batch N
    |
    v
Pipeline::with_negrisk().solve(&problem)
    |
    +-- Iteration 1:
    |   +-- LocalSolver::discover_prices()
    |   |   `-- For each binary market: solve_binary_market_unified()
    |   |       +-- Classify YES/NO buyers and sellers
    |   |       +-- Convert NO buyers -> YES supply at ($1 - limit)
    |   |       +-- Find unified clearing price P_YES
    |   |       `-- P_NO = $1 - P_YES
    |   +-- NegriskSolver::find_arbitrage()
    |   |   +-- Sum YES prices per MarketGroup
    |   |   +-- Create arb orders with fair-share limits
    |   |   `-- Store for next iteration
    |   +-- MmAllocator::allocate()
    |   |   `-- Filter fills to MM budget constraints
    |   `-- ArbitrageDetector (bundle matching)
    |
    +-- Iteration 2..5:
    |   +-- LocalSolver sees arb orders from previous iteration
    |   |   `-- Arb NO-buy orders become YES supply -> prices adjust
    |   +-- NegriskSolver recalculates with updated prices
    |   `-- ...converging toward sum=$1
    |
    `-- Return fills, prices, welfare
```

---

## Bugs that were fixed

### 1. Negrisk payoff swap

**Before:** `order.payoffs[0] = payoff_no; order.payoffs[1] = payoff_yes;`

State 0 = YES, state 1 = NO (consistent with `simple_yes_buy()` which sets
`payoff_at(0, 1)` for YES). The assignment put YES payoff in the NO slot and
vice versa. Posrisk "buy NO" actually created YES-buy orders. Negrisk "buy YES"
actually created NO-buy orders.

This was invisible with per-outcome clearing (arb orders had no cross-outcome
effect anyway). With unified clearing it would push prices the wrong way.

**After:** `order.payoffs[0] = yes_payoff; order.payoffs[1] = no_payoff;`

### 2. Current-price limits

**Before:** `limit_price = current_market_price` (first order got inflated limit
for welfare attribution; others had zero-welfare limits).

Arb orders at current prices convert to supply/demand AT the existing clearing
price. Zero price pressure. Prices don't move.

**After:** `limit_price = fair_share_price` (proportional scaling to sum=$1).
Each order is willing to pay up to the fair value. In unified clearing, this
creates supply below or demand above the current clearing, moving prices.

### 3. Per-outcome clearing

**Before:** YES and NO cleared in separate independent auctions. A NO buyer was
invisible to YES clearing. Negrisk NO-buy arb orders had zero effect on YES
prices.

**After:** One auction per binary market. NO buyers convert to YES supply. The
clearing sees all participants and finds a single price.

---

## Formal Properties

An honest assessment of what this approach guarantees and what it doesn't.

### What IS guaranteed

**Within each binary market:**
- P_YES + P_NO = $1 (exact, by construction)
- Clearing maximizes matched volume for a given supply-demand structure
- All fills respect limit prices (buyers pay <= limit, sellers receive >= limit)
- Per-market welfare is non-negative (each fill has non-negative surplus)

**Negrisk detection:**
- Correctly identifies when sum(YES) != $1
- Profit calculation is correct: $1 - sum for negrisk, sum - $1 for posrisk
- Fair-share limits sum to exactly $1 across the group

### What is NOT guaranteed

**Global welfare optimality:** each market is cleared independently. The joint
clearing across all markets in a group is not solved as one optimization. A
joint LP would find a better or equal solution. The iterative approach is a
heuristic decomposition.

**Convergence:** the fixed-point iteration is not proven to converge. It runs
for a fixed 5 iterations and stops. Empirically it converges to within ~2% of
sum=$1, but there is no formal proof that it always does, or that it converges
monotonically.

**Atomicity of arb orders:** the arb legs are independent single-market orders.
If only some legs fill, the synthetic arbitrageur has an exposed position. This
doesn't affect real participants (the arb is internal to the solver), but it
means the arb welfare accounting may be inaccurate.

**Conservation across arb legs:** since arb orders are not atomic, the "buy NO
on all outcomes" strategy may partially execute, which doesn't correspond to a
real hedged position. The fills are individually valid (each respects its
market's clearing) but collectively they may not represent a coherent trade.

**Last-iteration welfare loss:** the final iteration's arb orders never enter
a subsequent price discovery pass, so their price-correction effect is lost.
This is a small welfare loss accepted for soundness (no synthetic fills are
injected).

### Comparison to the optimal approach

The theoretically correct approach is a single constrained optimization:

```
maximize   sum_i  welfare(fill_i)
subject to:
  for each market m:
    sum(buy_qty_m) = sum(sell_qty_m)          [supply-demand balance]
    clearing_price_m >= seller_limit           [seller constraint]
    clearing_price_m <= buyer_limit            [buyer constraint]
  for each MarketGroup g:
    sum_{m in g} clearing_price_YES_m = $1     [price consistency]
  for each MM constraint:
    sum(capital_used) <= budget                 [MM budget]
```

This is a linear program. It finds the jointly optimal fills and prices.

Our iterative approach decomposes this into:
- Per-market subproblems (LocalSolver) — handles supply-demand balance and
  limit constraints
- Cross-market coordination (NegriskSolver) — approximates the price consistency
  constraint through synthetic demand/supply
- MM coordination (MmAllocator) — handles budget constraints

This resembles **dual decomposition** or **Dantzig-Wolfe decomposition** in
operations research, where a hard problem is split into easy subproblems
coordinated through price signals (Lagrange multipliers).

The fair-share limit prices act as approximate dual variables for the sum=$1
constraint. In a proper dual decomposition, you'd update these multipliers using
subgradient information. Our approach uses proportional scaling instead, which is
a specific (and not necessarily optimal) update rule.

### What a formal analysis would require

To write a paper with proofs:

1. **Formulate the joint LP** and its dual
2. **Show the iterative scheme** as a specific decomposition of the LP
3. **Prove convergence** under conditions on the order book (e.g., sufficient
   liquidity, bounded price ranges). Likely requires showing the fair-share
   update is a contraction mapping or satisfies sufficient decrease conditions.
4. **Bound the welfare gap** between the iterative solution and the LP optimum
5. **Address atomicity** by either making arb orders atomic (bundle orders) or
   proving that partial execution still moves prices in the right direction

---

## Mechanism 3: Dual Decomposition (New)

**File:** `dual_master.rs` + `pipeline.rs` (via `Pipeline::with_dual_decomposition()`)

### What it does

Replaces the ad-hoc NegriskSolver + MmAllocator pipeline with a principled
**Lagrangian dual decomposition** framework. Coupling constraints (sum=$1
across MarketGroups, MM budget limits) are relaxed into penalty terms. Per-market
subproblems are solved independently, and dual variables are updated via
subgradient descent.

### Why it exists

The NegriskSolver approach (Mechanism 2) has no convergence guarantee and
uses fair-share pricing as a heuristic approximation of dual variables.
The MmAllocator does greedy post-hoc filtering instead of incorporating budget
constraints into the clearing itself. Dual decomposition addresses both
shortcomings.

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
  |   |   +-- 3. compute_primal_residuals(): constraint violations
  |   |   +-- 4. update_duals(): subgradient step on λ, μ
  |   |   +-- 5. check_convergence(): primal + dual tolerance
  |   |
  |   +-- Final pass: re-solve with converged shading, validate
  |   |   against original limits and MM budgets
  |   |
  |   +-- Return fills, prices, convergence diagnostics
  |
  +-- ArbitrageDetector: bundle/spread matching on remaining liquidity
  |
  +-- Return combined PipelineResult
```

### Bid Shading Math

The Lagrangian relaxes two constraint types:

**Price Consistency (λ):** For MarketGroup g with constraint Σ P_YES_i = $1:
- λ > 0 (sum > $1): YES buyers bid less, YES supply increases → prices drop
- λ < 0 (sum < $1): YES buyers bid more, YES supply decreases → prices rise

Shading formulas:
- YES buyer:  `effective = limit - λ × $1`
- NO buyer:   `effective = limit + λ × $1`
- YES seller: `effective = limit - λ × $1`
- NO seller:  `effective = limit + λ × $1`

**Pacing (μ):** For MM k with budget constraint Σ capital ≤ B_k:
- μ > 0 (over budget): MM bids less aggressively

Shading formulas (MM orders only):
- BuyYes/BuyNo:  `paced = limit / (1 + μ)`
- SellYes/SellNo: `paced = (limit + μ × $1) / (1 + μ)`

Both adjustments compose: pacing first, then price consistency.

### Convergence

- Step size: α_t = α_0 / √t (diminishing, standard for subgradient)
- Primal tolerance: 2% of $1 for price sum, 2% for budget
- Dual tolerance: 0.1% change in dual variables

The subgradient method guarantees convergence to within ε of optimal for
convex problems with diminishing step sizes. Our per-market subproblems
are piecewise-linear (LP), so the Lagrangian dual is convex.

### Comparison to Mechanism 2 (NegriskSolver)

| Property | NegriskSolver | Dual Decomposition |
|----------|--------------|-------------------|
| Price consistency | Heuristic (fair-share) | Subgradient (principled) |
| MM budgets | Post-hoc greedy | Bid shading (in-clearing) |
| Convergence | No guarantee | Subgradient guarantee |
| Implementation | Synthetic orders | Dual variables |
| Multi-market orders | Same (ArbitrageDetector) | Same (ArbitrageDetector) |

Both pipelines remain available for comparison:
- `Pipeline::with_negrisk()` — existing approach
- `Pipeline::with_dual_decomposition()` — new approach

### Limitations

**Multi-market orders** are still handled as a post-processing phase
(ArbitrageDetector after dual convergence). Bundle/spread fills don't feed
back into the dual decomposition, so their price impact is not captured in
the equilibrium. This is standard in combinatorial auctions.

**Duality gap**: the subgradient method finds the dual optimum, but the primal
recovery (mapping dual variables to actual fills) may have a gap. The final
pass validates all fills against original limits and MM budgets.
