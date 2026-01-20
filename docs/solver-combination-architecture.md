# Solver Combination Architecture

## Problem Statement

We have multiple specialized solvers, each with different strengths:
- **Base solver**: Standard LP matching
- **Arb solver**: Detects cross-market arbitrage
- **Synthetic solver**: Constructs synthetic positions from order combinations
- **MM solver**: Handles budget-constrained market makers
- **Future solvers**: Unknown specializations

**Question**: How do we combine their outputs to maximize total welfare?

**Constraints**:
- Solvers may run in TEE (trusted execution environment)
- We want best algorithm to win, not best infrastructure
- Solutions must be valid (no constraint violations)
- Combination should be welfare-maximizing

---

## Architecture Options

### Option A: Sequential Slot Auction

**Mechanism**:
```
1. Solvers bid for execution slots
2. Slot 1 (most valuable): highest bidder, runs first
3. Slot 2: second-highest bidder, runs on remaining unfilled orders
4. Slot N: Nth bidder, runs on whatever is left
5. Each slot has fixed time budget (e.g., 100ms)
```

**What each solver sees**:
- Slot 1: Full order book, all liquidity
- Slot 2: Unfilled orders from slot 1, remaining liquidity
- Slot N: Progressively smaller problem

**Bid dynamics**:
- Slot 1 is most valuable (most opportunity)
- Specialized solvers might bid high for specific order patterns
- Generalist solvers bid for early slots
- Specialists might prefer later slots if their niche orders aren't filled

**Example**:
```
Orders: [A, B, C, D, E, F] (6 orders)
Liquidity: $10,000 across markets

Slot 1 (Generalist): Fills [A, B, C] using $6,000 liquidity
Slot 2 (Arb specialist): Sees [D, E, F], $4,000 remaining
         Detects arb between D and E, fills both
Slot 3 (Synthetic): Sees [F], $2,000 remaining
         Can't help with F alone, passes
```

**Pros**:
- Simple to implement
- Clear incentive alignment (bid = expected value capture)
- No merge complexity
- TEE ensures fair compute

**Cons**:
- Later slots see depleted state
- Cross-slot synergies missed (slot 2 can't improve slot 1's fills)
- Slot 1 winner has disproportionate advantage

**Variant A': Slot auction with improvement rights**
```
After all slots execute:
- Any solver can propose improvements to ANY fill
- Improvement = higher welfare for same orders
- Improver pays fee, captures fraction of welfare gain
```

This adds a second phase where cross-slot optimization can happen.

---

### Option B: Sequential Patches (First Valid Wins)

**Mechanism**:
```
1. Base solver produces initial solution S₀
2. All solvers race to find improvements (patches)
3. First valid, welfare-improving patch wins
4. Apply patch → S₁
5. Repeat until timeout or no improvements
```

**What is a patch?**
```rust
struct Patch {
    // Fills to ADD
    new_fills: Vec<Fill>,
    // Fills to REMOVE (if improving existing)
    removed_fills: Vec<FillId>,
    // Expected welfare delta
    welfare_delta: i64,
}
```

**Validation**:
```rust
fn validate_patch(current: &Solution, patch: &Patch) -> bool {
    // 1. Removed fills must exist in current solution
    // 2. New fills must satisfy order constraints
    // 3. Combined fills must not exceed liquidity
    // 4. welfare_delta must be accurate and positive
}
```

**Example**:
```
S₀: [Fill(A, $100), Fill(B, $200)]  Welfare: $50

Solver X finds: "Fill C instead of B gives welfare $80"
Patch X: { new_fills: [Fill(C, $200)], removed: [Fill(B)], delta: +$30 }

Solver Y finds: "Fill D alongside A gives welfare $70"
Patch Y: { new_fills: [Fill(D, $150)], removed: [], delta: +$20 }

X submits first → X wins → S₁ includes C instead of B
Y's patch may still be valid on S₁ (if D doesn't conflict with C)
```

**Pros**:
- Best algorithm wins (TEE = fair compute)
- Incremental improvement guaranteed
- Simple validation
- Natural convergence (improvements get smaller)

**Cons**:
- First-mover advantage for easy improvements
- Complex improvements might lose to simple ones
- Race condition fairness depends on TEE timing guarantees

**Key insight**: In TEE, "first" means "fastest algorithm", which is what we want. No infrastructure advantage.

---

### Option C: Parallel Patches with Merge

**Mechanism**:
```
1. Base solver produces S₀
2. All solvers run in parallel, each produces patches
3. Collect all patches at deadline
4. Merge compatible patches
5. Resolve conflicts by welfare
```

**Patch independence**:

Two patches are INDEPENDENT if they don't share:
- Orders (neither fills the same order)
- Liquidity (don't compete for same price levels)
- Constraints (don't both use same MM budget, etc.)

```rust
fn patches_independent(p1: &Patch, p2: &Patch) -> bool {
    // No shared orders
    let p1_orders: HashSet<_> = p1.touched_orders().collect();
    let p2_orders: HashSet<_> = p2.touched_orders().collect();
    if !p1_orders.is_disjoint(&p2_orders) {
        return false;
    }

    // No shared liquidity
    let p1_liquidity = p1.liquidity_used();
    let p2_liquidity = p2.liquidity_used();
    if liquidity_overlaps(&p1_liquidity, &p2_liquidity) {
        return false;
    }

    // No shared constraints
    // ... (MM budgets, etc.)

    true
}
```

**Merge algorithm**:
```rust
fn merge_patches(patches: Vec<Patch>) -> Vec<Patch> {
    // Sort by welfare descending
    let sorted = patches.sorted_by(|a, b| b.welfare_delta.cmp(&a.welfare_delta));

    let mut accepted: Vec<Patch> = vec![];

    for patch in sorted {
        // Check if compatible with all accepted patches
        if accepted.iter().all(|a| patches_independent(a, &patch)) {
            accepted.push(patch);
        } else {
            // Conflict - this patch loses (lower welfare)
        }
    }

    accepted
}
```

**The "price moved" question**:

If patch A fills orders in market X, prices change. Is patch B (also touching X) still valid?

```rust
fn patch_still_valid_at_new_prices(patch: &Patch, new_prices: &Prices) -> bool {
    for fill in &patch.new_fills {
        let order = get_order(fill.order_id);
        let new_price = new_prices.get(fill.market, fill.outcome);

        if order.is_buy() && new_price > order.limit_price {
            return false;  // Price moved against buyer
        }
        if order.is_sell() && new_price < order.limit_price {
            return false;  // Price moved against seller
        }
    }
    true
}
```

**Key insight**: A patch might be valid at new prices even if prices moved, as long as the new price is still within limits. This is GOOD - it means more patches can be combined.

**Example**:
```
Market X: clearing price $0.50

Patch A: Fill buy order (limit $0.60) at $0.50
         → Price moves to $0.52

Patch B: Fill buy order (limit $0.55) at $0.50
         → At new price $0.52, still valid! (0.52 < 0.55)

Both patches can be accepted.
```

**Pros**:
- Parallel = fair (no first-mover advantage)
- Independent patches combine optimally
- Price movement doesn't kill valid patches

**Cons**:
- Merge is complex
- Many patches may conflict in practice
- Need to re-validate at new prices

---

### Option D: LP Contribution Model

**Radical idea**: Solvers don't produce SOLUTIONS. They produce PROBLEM COMPONENTS.

**Base LP**:
```
maximize: Σ (limit_price - fill_price) × fill_qty  [buyer welfare]
        + Σ (fill_price - limit_price) × fill_qty  [seller welfare]

subject to:
    fill_qty ≤ max_qty                             [order limits]
    fill_price ≤ limit_price (buyers)              [price limits]
    fill_price ≥ limit_price (sellers)
    Σ fills_at_price ≤ liquidity_at_price          [liquidity]
```

**Solver contributions**:

Each solver can ADD to the LP:

1. **New Variables**:
   ```
   Arb solver: "Consider variable arb_position_X representing..."
   Synthetic solver: "Consider variable synthetic_Y = order_A + order_B..."
   ```

2. **New Constraints**:
   ```
   Arb solver: "Price(A) = Price(A∧B) + Price(A∧¬B)"  [consistency]
   MM solver: "Σ capital_used_by_MM_i ≤ budget_i"    [budget]
   ```

3. **New Objective Terms**:
   ```
   Solver X: "Add welfare term for synthetic positions"
   ```

**Combined LP**:
```
maximize: base_welfare + Σ solver_contributed_objectives

subject to:
    base_constraints
    + arb_solver_constraints
    + mm_solver_constraints
    + synthetic_solver_constraints
    + ...
```

**Example**:

```
Base problem: 3 orders in 2 markets

Arb solver contributes:
    Constraint: p_rain = p_RC + p_R¬C  [marginal consistency]

Synthetic solver contributes:
    Variable: synth_1 = order_A.fill + order_B.fill  [combined position]
    Constraint: synth_1 payoff = [1,1,0,0]
    Objective: + synth_welfare(synth_1)

MM solver contributes:
    Constraint: capital_MM1 ≤ $10,000
    Variable: mm1_fill_1, mm1_fill_2, ...

Combined LP solved ONCE → optimal solution considering ALL insights
```

**Pros**:
- OPTIMAL combination (not greedy)
- No merge needed - LP solver handles it
- Composable - solvers don't interfere
- Declarative - easy to verify contributions

**Cons**:
- Solvers must express logic as LP components
- Some heuristics can't be linearized
- Solver "secret sauce" is exposed (they reveal their constraints)
- LP might become large/slow

**When this works well**:
- Arb detection: naturally expressed as linear constraints
- Budget constraints: linear
- Consistency requirements: linear

**When this struggles**:
- Complex heuristics ("if X then Y else Z")
- ML-based solvers
- Proprietary algorithms that can't be decomposed

---

### Option E: Fixed-Point Iteration with Solver Layer

**Mechanism**:
```
Iteration 1:
    1. Run base solver → prices P₁, fills F₁
    2. Each solver proposes additional fills given P₁
    3. Validate and accept proposals
    4. Update state

Iteration 2:
    1. Run base solver with new state → prices P₂, fills F₂
    2. Solvers refine proposals given P₂
    3. ...

Repeat until convergence (prices stable)
```

**This is the current MM approach generalized**:
- MM solver = budget-constrained liquidity provider
- Other solvers = additional specialized optimizers

**Solver interface**:
```rust
trait IterativeSolver {
    /// Given current prices, propose fills
    fn propose_fills(&self, prices: &Prices, available_orders: &[Order]) -> Vec<Fill>;

    /// Update internal state after iteration
    fn update_state(&mut self, new_prices: &Prices, accepted_fills: &[Fill]);
}
```

**Convergence**:
- Each iteration, prices adjust based on fills
- Solvers adjust proposals based on prices
- Converges when no solver wants to change their proposal

**Pros**:
- Handles complex interactions
- Solvers can use any algorithm (black box)
- Natural extension of current architecture

**Cons**:
- May not converge (oscillation)
- Slow (multiple iterations)
- Order of solver execution within iteration matters

---

## Comparison Matrix

| Aspect | A: Slots | B: Sequential | C: Parallel Merge | D: LP Contrib | E: Fixed-Point |
|--------|----------|---------------|-------------------|---------------|----------------|
| Optimality | Greedy | Greedy | Near-optimal | Optimal | Local optimum |
| Complexity | Low | Medium | High | Medium | Medium |
| Solver flexibility | High | High | High | Low (must be LP) | High |
| Fairness | Auction-based | First-wins | Welfare-based | Equal | Iteration-based |
| Cross-solver synergy | None | Limited | Good | Perfect | Good |
| Convergence | Guaranteed | Guaranteed | Guaranteed | Guaranteed | Not guaranteed |
| Latency | O(N × slot_time) | O(improvements) | O(1 parallel) | O(LP solve) | O(iterations) |

---

## Hybrid Recommendation

**Phase 1: MVP**

Use **Option D (LP Contribution)** for core solvers:
- Arb detection → consistency constraints
- MM budget → budget constraints
- Synthetic matching → synthetic variables

All core logic expressed declaratively. Single LP solve. Optimal.

**Phase 2: External Solvers**

Add **Option C (Parallel Merge)** layer on top:
```
1. Solve core LP → baseline solution S₀
2. External solvers propose patches in parallel
3. Merge compatible patches (welfare-ranked)
4. Final solution = S₀ + merged patches
```

This allows:
- Core optimality from LP
- External innovation from black-box solvers
- Fair competition via parallel submission

**Phase 3: Dark Forest**

Add **Option A (Slot Auction)** for ordering:
```
1. Solvers bid for patch submission priority
2. Higher bidder's patches evaluated first in conflicts
3. Creates market for "solver attention"
```

This monetizes solver competition while maintaining fairness.

---

## Deep Dive: Patch Structure

A well-designed patch system is crucial for Options B, C, and the hybrid.

### Patch Definition

```rust
/// A patch represents a proposed modification to the current solution
struct Patch {
    /// Unique identifier
    id: PatchId,

    /// Solver that created this patch
    solver_id: SolverId,

    /// Orders this patch affects
    affected_orders: Vec<OrderId>,

    /// New fills to add
    new_fills: Vec<Fill>,

    /// Existing fills to remove (for improvements)
    removed_fills: Vec<FillId>,

    /// Markets/outcomes this patch touches
    touched_markets: Vec<(MarketId, Outcome)>,

    /// Liquidity consumed by this patch
    liquidity_delta: Vec<LiquidityDelta>,

    /// Claimed welfare improvement
    welfare_delta: i64,

    /// Proof of validity (optional, for complex patches)
    validity_proof: Option<ValidityProof>,
}

struct LiquidityDelta {
    market_id: MarketId,
    outcome: Outcome,
    price_level: Nanos,
    qty_consumed: Qty,
}
```

### Patch Validation

```rust
fn validate_patch(
    current_solution: &Solution,
    current_state: &MarketState,
    patch: &Patch,
) -> Result<ValidatedPatch, ValidationError> {

    // 1. Check removed fills exist
    for fill_id in &patch.removed_fills {
        if !current_solution.fills.contains(fill_id) {
            return Err(ValidationError::FillNotFound(*fill_id));
        }
    }

    // 2. Check new fills satisfy order constraints
    for fill in &patch.new_fills {
        let order = get_order(fill.order_id)?;

        // Quantity within limits
        if fill.fill_qty > order.max_fill || fill.fill_qty < order.min_fill {
            return Err(ValidationError::QuantityOutOfBounds);
        }

        // Price within limits
        let is_buy = order.payoffs.iter().any(|&p| p > 0);
        if is_buy && fill.fill_price > order.limit_price {
            return Err(ValidationError::PriceViolation);
        }
        if !is_buy && fill.fill_price < order.limit_price {
            return Err(ValidationError::PriceViolation);
        }
    }

    // 3. Check liquidity availability
    let available_liquidity = compute_available_liquidity(current_state, &patch.removed_fills);
    for delta in &patch.liquidity_delta {
        let available = available_liquidity.get(delta.market_id, delta.outcome, delta.price_level);
        if delta.qty_consumed > available {
            return Err(ValidationError::InsufficientLiquidity);
        }
    }

    // 4. Verify welfare calculation
    let actual_welfare_delta = compute_welfare_delta(current_solution, patch);
    if actual_welfare_delta != patch.welfare_delta {
        return Err(ValidationError::WelfareMismatch);
    }

    if actual_welfare_delta <= 0 {
        return Err(ValidationError::NotWelfareImproving);
    }

    Ok(ValidatedPatch { patch: patch.clone(), verified_welfare: actual_welfare_delta })
}
```

### Patch Independence Check

```rust
fn patches_independent(p1: &Patch, p2: &Patch) -> bool {
    // Check order independence
    let p1_orders: HashSet<_> = p1.affected_orders.iter().collect();
    let p2_orders: HashSet<_> = p2.affected_orders.iter().collect();
    if !p1_orders.is_disjoint(&p2_orders) {
        return false;
    }

    // Check liquidity independence
    for d1 in &p1.liquidity_delta {
        for d2 in &p2.liquidity_delta {
            if d1.market_id == d2.market_id
                && d1.outcome == d2.outcome
                && d1.price_level == d2.price_level
            {
                return false;  // Both consume same liquidity
            }
        }
    }

    // Check fill removal independence
    let p1_removes: HashSet<_> = p1.removed_fills.iter().collect();
    let p2_removes: HashSet<_> = p2.removed_fills.iter().collect();
    if !p1_removes.is_disjoint(&p2_removes) {
        return false;  // Both try to remove same fill
    }

    true
}
```

### Price-Adjusted Validity

```rust
/// Check if patch is still valid after prices changed
fn patch_valid_at_new_prices(
    patch: &Patch,
    old_prices: &Prices,
    new_prices: &Prices,
) -> bool {
    for fill in &patch.new_fills {
        let order = get_order(fill.order_id).unwrap();
        let new_clearing_price = new_prices.get(fill.market_id, fill.outcome);

        // For buyers: new price must still be ≤ limit
        // For sellers: new price must still be ≥ limit
        let is_buy = order.payoffs.iter().any(|&p| p > 0);

        if is_buy && new_clearing_price > order.limit_price {
            return false;
        }
        if !is_buy && new_clearing_price < order.limit_price {
            return false;
        }
    }

    // Recalculate welfare at new prices
    let new_welfare = compute_patch_welfare_at_prices(patch, new_prices);

    // Patch still valid if welfare is positive
    new_welfare > 0
}
```

---

## Deep Dive: LP Contribution Model

### Contribution Types

```rust
enum LPContribution {
    /// Add a new variable to the LP
    Variable {
        name: String,
        lower_bound: f64,
        upper_bound: f64,
        objective_coefficient: f64,  // Contribution to welfare
    },

    /// Add a new constraint
    Constraint {
        name: String,
        coefficients: Vec<(VariableName, f64)>,  // Linear combination
        relation: Relation,  // ≤, =, ≥
        rhs: f64,
    },

    /// Link a new variable to existing variables
    VariableLink {
        new_var: String,
        existing_vars: Vec<(VariableName, f64)>,  // new_var = Σ coef × existing
    },
}

enum Relation {
    LessOrEqual,
    Equal,
    GreaterOrEqual,
}
```

### Example: Arb Solver Contribution

```rust
fn arb_solver_contribute(markets: &[Market]) -> Vec<LPContribution> {
    let mut contributions = vec![];

    // Find related markets (marginal vs joint)
    for marginal in markets.iter().filter(|m| m.is_marginal()) {
        let related_joints = find_joint_markets_for(marginal);

        if !related_joints.is_empty() {
            // Add consistency constraint:
            // P(marginal) = Σ P(joint_i) for all joints that imply marginal

            let mut coefficients = vec![];
            coefficients.push((format!("price_{}", marginal.id), 1.0));

            for joint in &related_joints {
                coefficients.push((format!("price_{}", joint.id), -1.0));
            }

            contributions.push(LPContribution::Constraint {
                name: format!("arb_consistency_{}", marginal.id),
                coefficients,
                relation: Relation::Equal,
                rhs: 0.0,
            });
        }
    }

    contributions
}
```

### Example: Synthetic Solver Contribution

```rust
fn synthetic_solver_contribute(orders: &[Order]) -> Vec<LPContribution> {
    let mut contributions = vec![];

    // Find order combinations that create useful synthetics
    for (i, o1) in orders.iter().enumerate() {
        for o2 in orders.iter().skip(i + 1) {
            if let Some(synthetic) = can_combine(o1, o2) {
                // Add variable for synthetic fill
                let var_name = format!("synth_{}_{}", o1.id, o2.id);

                contributions.push(LPContribution::Variable {
                    name: var_name.clone(),
                    lower_bound: 0.0,
                    upper_bound: synthetic.max_qty as f64,
                    objective_coefficient: synthetic.welfare_per_unit,
                });

                // Link synthetic to component orders
                contributions.push(LPContribution::VariableLink {
                    new_var: var_name,
                    existing_vars: vec![
                        (format!("fill_{}", o1.id), 1.0),
                        (format!("fill_{}", o2.id), 1.0),
                    ],
                });
            }
        }
    }

    contributions
}
```

### Example: MM Solver Contribution

```rust
fn mm_solver_contribute(mms: &[MarketMaker]) -> Vec<LPContribution> {
    let mut contributions = vec![];

    for mm in mms {
        // Add budget constraint for this MM
        let mut coefficients = vec![];

        for order in &mm.orders {
            // Capital used = price × qty for buys, (1-price) × qty for sells
            let capital_coef = if order.is_buy() {
                order.limit_price as f64 / NANOS_PER_DOLLAR as f64
            } else {
                1.0 - (order.limit_price as f64 / NANOS_PER_DOLLAR as f64)
            };

            coefficients.push((format!("fill_{}", order.id), capital_coef));
        }

        contributions.push(LPContribution::Constraint {
            name: format!("mm_budget_{}", mm.id),
            coefficients,
            relation: Relation::LessOrEqual,
            rhs: mm.budget as f64,
        });
    }

    contributions
}
```

### LP Assembly

```rust
fn build_combined_lp(
    base_problem: &MatchingProblem,
    contributions: Vec<Vec<LPContribution>>,
) -> LP {
    let mut lp = build_base_lp(base_problem);

    for solver_contributions in contributions {
        for contribution in solver_contributions {
            match contribution {
                LPContribution::Variable { name, lower_bound, upper_bound, objective_coefficient } => {
                    lp.add_variable(&name, lower_bound, upper_bound, objective_coefficient);
                }
                LPContribution::Constraint { name, coefficients, relation, rhs } => {
                    lp.add_constraint(&name, &coefficients, relation, rhs);
                }
                LPContribution::VariableLink { new_var, existing_vars } => {
                    // new_var = Σ coef × existing
                    // becomes: new_var - Σ coef × existing = 0
                    let mut coefficients = vec![(new_var, 1.0)];
                    for (var, coef) in existing_vars {
                        coefficients.push((var, -coef));
                    }
                    lp.add_constraint(&format!("link_{}", new_var), &coefficients, Relation::Equal, 0.0);
                }
            }
        }
    }

    lp
}
```

---

## Recommended Architecture

### Final Design: Layered Hybrid

```
┌─────────────────────────────────────────────────┐
│                 Order Flow                       │
└─────────────────────┬───────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────┐
│         Layer 1: LP Contribution                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │   Arb    │ │ Synthetic│ │    MM    │        │
│  │  Solver  │ │  Solver  │ │  Solver  │        │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘        │
│       │            │            │               │
│       ▼            ▼            ▼               │
│  ┌─────────────────────────────────────────┐   │
│  │         Combined LP Solver               │   │
│  │  (Optimal solution for declarative      │   │
│  │   solvers)                              │   │
│  └────────────────┬────────────────────────┘   │
└───────────────────┼─────────────────────────────┘
                    │ Baseline Solution S₀
                    ▼
┌─────────────────────────────────────────────────┐
│         Layer 2: Patch Competition              │
│                                                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │ External │ │ External │ │ External │        │
│  │ Solver A │ │ Solver B │ │ Solver C │        │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘        │
│       │            │            │               │
│       ▼            ▼            ▼               │
│  ┌─────────────────────────────────────────┐   │
│  │         Parallel Patch Submission        │   │
│  │  (All submit within deadline)           │   │
│  └────────────────┬────────────────────────┘   │
│                   │                             │
│                   ▼                             │
│  ┌─────────────────────────────────────────┐   │
│  │         Patch Merge                      │   │
│  │  - Validate all patches                 │   │
│  │  - Check independence                   │   │
│  │  - Combine compatible patches           │   │
│  │  - Resolve conflicts by welfare         │   │
│  └────────────────┬────────────────────────┘   │
└───────────────────┼─────────────────────────────┘
                    │ Final Solution S_final
                    ▼
┌─────────────────────────────────────────────────┐
│              Settlement                          │
└─────────────────────────────────────────────────┘
```

### Why This Design?

1. **Layer 1 (LP)** handles core matching optimally
   - Arb detection is naturally linear
   - MM budgets are naturally linear
   - Synthetic matching is naturally linear
   - Single optimal solution, no merge needed

2. **Layer 2 (Patches)** enables external innovation
   - Black-box solvers can participate
   - Proprietary algorithms protected
   - Fair competition via parallel submission
   - Only improvements over LP baseline accepted

3. **Separation of concerns**
   - LP handles "known" optimization problems
   - Patches handle "unknown" opportunities
   - Clear interface between layers

### Implementation Priority

1. **Now**: Implement LP layer with arb + MM contributions
2. **Next**: Add patch interface for external solvers
3. **Later**: Add slot auction for patch priority (dark forest mode)

---

## Open Questions

1. **Patch fee structure**: Should solvers pay to submit patches? How much?

2. **Welfare sharing**: If a patch improves welfare by $100, who gets it?
   - All to users? (Altruistic)
   - Split with solver? (Incentive)
   - Auction the improvement rights?

3. **TEE timing guarantees**: How do we ensure "parallel" is actually parallel in TEE?

4. **LP contribution validation**: How do we verify solver contributions don't break the LP?

5. **Patch spam**: How do we prevent solvers from submitting many low-quality patches?
