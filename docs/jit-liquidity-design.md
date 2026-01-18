# JIT Liquidity: Design Specification

## Corrected Understanding

### FBA Changes the Adverse Selection Game

In CLOB:
- MM quotes bid 0.48, ask 0.52
- Informed trader hits the ask at 0.52
- Price immediately moves to 0.60
- MM is stuck holding at 0.52, market is at 0.60 → "picked off"

In FBA:
- MM quotes bid 0.48, ask 0.52
- Informed trader submits buy
- Batch clears at 0.55 (informed flow moves price)
- MM sells at 0.55, not 0.52 → **gets fair price!**

**Key insight**: FBA protects MMs from getting picked off because everyone gets the clearing price. The "adverse selection" in FBA is different - it's about BEING on the wrong side, not about getting a bad price.

### Flash Quoting ≠ Capital Locking

I was confused earlier. Let me clarify:

**Traditional limit orders**: Lock capital at limit price (linear constraint)
**Flash quoting (bilinear)**: Share budget across orders, only lock for expected fills
**JIT**: Provide liquidity after seeing orderbook, lock nothing until matched

Flash quoting and JIT are BOTH capital efficient. The difference is TIMING:
- Flash: Commit before seeing orderbook
- JIT: Commit after seeing orderbook

### Last Look in FX

Yes, this is real. In FX:
- Bank quotes price to client
- Client "hits" the quote
- Bank has ~50-200ms to accept or reject
- Bank can reject if price moved against them

Not SEC regulation per se - it's market convention / bilateral agreements. Very controversial. Some jurisdictions pushing back.

**We don't want last look** - it's unfair to traders. But JIT is different: MM provides NEW liquidity, doesn't reject existing trades.

---

## Mainline JIT Design

### Core Principles

1. **JIT must improve welfare** - can't just insert for free
2. **Fee prevents excessive JIT** - dynamic, EIP-1559 style
3. **Affected parties get rebates** - fair to those displaced
4. **Blind auction** - prevents JIT races

### The JIT Flow

```
1. Batch seals with user orders
2. Base solution computed (single-market, no JIT)
3. JIT window opens (~200ms)
4. MMs submit sealed JIT bids (order + fee offer)
5. JIT window closes
6. JIT bids revealed, evaluated
7. Winning JIT orders added to solution
8. Final solution computed
9. Batch clears
```

### JIT Bid Structure

```rust
struct JITBid {
    // The liquidity being provided
    orders: Vec<Order>,

    // How much MM is willing to pay for inclusion
    fee_bid: Decimal,

    // Commitment (for blind auction)
    commitment: Hash,  // hash(orders || fee_bid || nonce)
}

struct Order {
    market_id: MarketId,
    side: Side,
    size: Decimal,
    limit_price: Decimal,
}
```

### Welfare Requirement

JIT is only accepted if it improves total welfare by minimum threshold:

```rust
fn evaluate_jit(base_solution: &Solution, jit_bid: &JITBid) -> Option<JITResult> {
    // Compute solution with JIT orders added
    let new_solution = solve_with_jit(base_solution, &jit_bid.orders);

    // Welfare delta
    let welfare_delta = new_solution.welfare - base_solution.welfare;

    // Must improve welfare by at least min_threshold
    let min_threshold = compute_min_threshold(&jit_bid);
    if welfare_delta < min_threshold {
        return None;  // Reject: not enough welfare improvement
    }

    // Check fee is sufficient
    let required_fee = compute_required_fee(welfare_delta);
    if jit_bid.fee_bid < required_fee {
        return None;  // Reject: fee too low
    }

    Some(JITResult {
        welfare_delta,
        fee_paid: jit_bid.fee_bid,
        affected_users: compute_affected_users(base_solution, new_solution),
    })
}
```

### Fee Mechanism (EIP-1559 Style)

Dynamic fee that adjusts based on JIT activity:

```rust
struct JITFeeState {
    base_fee: Decimal,      // Current base fee rate
    target_jit_ratio: Decimal,  // Target: JIT volume / total volume (e.g., 0.3)
}

impl JITFeeState {
    fn update(&mut self, batch_stats: &BatchStats) {
        let actual_ratio = batch_stats.jit_volume / batch_stats.total_volume;

        // If too much JIT, raise fee; if too little, lower fee
        if actual_ratio > self.target_jit_ratio {
            // Too much JIT, increase fee
            self.base_fee *= 1.125;  // +12.5% like EIP-1559
        } else {
            // Room for more JIT, decrease fee
            self.base_fee *= 0.875;  // -12.5%
        }

        // Clamp to reasonable range
        self.base_fee = self.base_fee.clamp(MIN_FEE, MAX_FEE);
    }

    fn compute_required_fee(&self, welfare_delta: Decimal) -> Decimal {
        // Fee = base_fee_rate × welfare improvement
        self.base_fee * welfare_delta
    }
}
```

**Why EIP-1559 style?**
- Self-regulating: fee finds equilibrium
- Predictable: MMs can estimate required fee
- Prevents JIT domination: if too much JIT, fee rises
- Allows JIT growth: if too little JIT, fee falls

**Parameters**:
- `target_jit_ratio`: 0.30 (30% of volume from JIT seems reasonable)
- `MIN_FEE`: 0.05 (5% of welfare)
- `MAX_FEE`: 0.50 (50% of welfare)
- Initial `base_fee`: 0.20 (20% of welfare)

### Rebate Distribution

When JIT affects other users, rebates compensate:

```rust
struct AffectedUser {
    user_id: UserId,
    impact: Impact,
}

enum Impact {
    // User got better price due to JIT
    PriceImprovement { amount: Decimal },

    // User got worse fill (less volume or worse price)
    NegativeImpact { amount: Decimal },

    // User was displaced entirely
    Displaced { would_have_received: Decimal },
}

fn distribute_rebates(jit_result: &JITResult) {
    let total_fee = jit_result.fee_paid;

    // Protocol takes base portion
    let protocol_share = total_fee * PROTOCOL_FEE_SHARE;  // e.g., 30%
    let rebate_pool = total_fee - protocol_share;

    // Distribute to negatively affected users proportionally
    let total_negative_impact: Decimal = jit_result.affected_users
        .iter()
        .filter_map(|u| match &u.impact {
            Impact::NegativeImpact { amount } => Some(*amount),
            Impact::Displaced { would_have_received } => Some(*would_have_received),
            _ => None,
        })
        .sum();

    for user in &jit_result.affected_users {
        match &user.impact {
            Impact::NegativeImpact { amount } => {
                let rebate = rebate_pool * (*amount / total_negative_impact);
                credit_user(user.user_id, rebate);
            }
            Impact::Displaced { would_have_received } => {
                let rebate = rebate_pool * (*would_have_received / total_negative_impact);
                credit_user(user.user_id, rebate);
            }
            Impact::PriceImprovement { .. } => {
                // User benefited, no rebate needed
            }
        }
    }
}
```

**Fee distribution**:
- 30% to protocol
- 70% to negatively affected users (rebates)

### Blind Auction for JIT Slots

To prevent JIT races and information games:

```
Phase 1: Commitment (blind)
  - MMs submit hash(orders || fee || nonce)
  - Nobody sees actual bids

Phase 2: Reveal
  - MMs reveal (orders, fee, nonce)
  - Verify hash matches commitment

Phase 3: Selection
  - Evaluate all valid JIT bids
  - Select set that maximizes (welfare_improvement - overlap_penalty)
```

```rust
fn select_jit_bids(bids: Vec<JITBid>, base_solution: &Solution) -> Vec<JITBid> {
    // Score each bid
    let mut scored: Vec<(JITBid, Decimal)> = bids
        .into_iter()
        .filter_map(|bid| {
            let result = evaluate_jit(base_solution, &bid)?;
            // Score = welfare delta (fee already checked in evaluate_jit)
            Some((bid, result.welfare_delta))
        })
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Greedily select non-conflicting bids
    let mut selected = Vec::new();
    let mut affected_markets = HashSet::new();

    for (bid, _score) in scored {
        let bid_markets: HashSet<_> = bid.orders.iter()
            .map(|o| o.market_id)
            .collect();

        // Check for conflict with already selected
        if bid_markets.is_disjoint(&affected_markets) {
            affected_markets.extend(bid_markets);
            selected.push(bid);
        }
    }

    selected
}
```

---

## Integration with LP Solver

### How JIT Fits the Solving Pipeline

```
┌─────────────────────────────────────────────────────────┐
│                    Batch Solving Pipeline                │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  1. User orders sealed                                   │
│          ↓                                               │
│  2. Base solution (single-market LP, no JIT)            │
│          ↓                                               │
│  3. Cross-market patches from solvers                    │
│          ↓                                               │
│  4. JIT window: MMs submit sealed bids                   │
│          ↓                                               │
│  5. JIT bids revealed and evaluated                      │
│          ↓                                               │
│  6. Final LP solve with:                                 │
│     - User orders                                        │
│     - Selected patches                                   │
│     - Selected JIT orders                                │
│          ↓                                               │
│  7. Validation and execution                             │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

### JIT Orders in LP Formulation

JIT orders are just regular orders in the LP, but with different accounting:

```
Standard LP:

Variables:
  p_m = clearing price for market m
  f_i = fill amount for order i

Objective:
  maximize Σ (user_value_i - p_m) × f_i   [buyer surplus]
         + Σ (p_m - user_cost_i) × f_i    [seller surplus]

With JIT orders added:

  maximize Σ (user_value_i - p_m) × f_i        [user surplus]
         + Σ (p_m - mm_cost_j) × f_j          [MM surplus from JIT]
         - Σ jit_fee_j                         [JIT fees paid]
```

The LP solver doesn't care that some orders are JIT - they're just orders. The JIT evaluation happens BEFORE adding to LP to ensure welfare improvement.

### JIT and Cross-Market Patches

JIT can interact with patches:

**Scenario**: Solver patch fills cross-market order by adjusting prices. JIT MM sees opportunity to provide additional liquidity at new prices.

**Handling**:
1. Compute base solution
2. Apply patches → intermediate solution
3. JIT window: MMs see intermediate solution (with patches)
4. JIT evaluated against intermediate solution
5. Final solution = intermediate + JIT

```rust
fn full_solve(orderbook: &Orderbook, patches: &[Patch], jit_bids: &[JITBid]) -> Solution {
    // Step 1: Base (single-market)
    let base = solve_single_markets(orderbook);

    // Step 2: Apply patches
    let with_patches = apply_patches(base, patches);

    // Step 3: Evaluate and select JIT
    let selected_jit = select_jit_bids(jit_bids, &with_patches);

    // Step 4: Final solve with everything
    let jit_orders: Vec<Order> = selected_jit
        .iter()
        .flat_map(|bid| bid.orders.clone())
        .collect();

    let final_orderbook = orderbook.clone().add_orders(jit_orders);
    solve_full_lp(&final_orderbook, &with_patches)
}
```

---

## Edge Cases and Considerations

### Empty/Thin Markets

**Problem**: On thin market, revealing orderbook lets adversary pick off MM.

**Example**:
```
Market has no orders
MM quotes: bid 0.40, ask 0.60 (wide spread due to thin market)
Adversary knows true value is 0.70
Adversary submits: buy at 0.65

In CLOB: Adversary gets filled at 0.60, MM is picked off
In FBA: Both get clearing price ~0.62, MM is less picked off

But if adversary can see MM's quote after seal...
They know they can buy at ~0.60
```

**With full privacy**: MM quote is hidden, adversary doesn't know the spread.

**With orderbook revealed to solvers**: Solver sees MM quote. If solver can trade (or leak info), privacy is broken.

**Solution**:
- Solvers cannot trade in same batch (separation)
- Solver information is not published
- MM quotes from previous batches are not revealed (only current batch structure)

This limits the attack to: adversary iteratively probes over many batches. Slow and detectable.

### JIT Crowding Out Passive Liquidity

**Concern**: If JIT is too good, nobody provides passive liquidity.

**Mitigation**: EIP-1559 fee mechanism
- If JIT dominates (>30% of volume), fee rises
- Rising fee makes passive liquidity competitive again
- Equilibrium: mix of passive and JIT

### Multiple JIT Bids Conflicting

**Problem**: MM_A and MM_B both want to provide liquidity in Market X.

**Solution**: Blind auction + greedy selection
- Both submit sealed bids
- Higher welfare improvement wins
- Or: could allow both if they don't conflict (different price levels)

### JIT Gaming the Welfare Metric

**Problem**: MM submits JIT that technically improves welfare but is extractive.

**Example**:
```
Base: Alice buys 100 @ 0.55, Bob sells 100 @ 0.45, clear at 0.50
Welfare = (0.55 - 0.50) × 100 + (0.50 - 0.45) × 100 = $10

MM JIT: Sell 100 @ 0.49
New clear: ~0.48
Alice welfare = (0.55 - 0.48) × 100 = $7 (better!)
Bob welfare = (0.48 - 0.45) × 50 = $1.50 (worse! partial fill)
MM welfare = (0.48 - MM_cost) × 100

Total welfare might be higher, but Bob got screwed.
```

**Solution**: Rebates
- Bob is "negatively affected"
- Bob gets rebate from JIT fee
- If rebate > Bob's loss, Bob is made whole

### JIT and Flash Quoting Coexistence

You said hybrid doesn't make sense. I agree:

- Flash quoting (bilinear) = capital-efficient passive quotes
- JIT = capital-efficient reactive quotes
- Both solve the same problem (capital efficiency)
- JIT is strictly more flexible (sees orderbook first)

**Recommendation**: JIT only, no separate "flash quoting" mechanism.

Users who want to provide passive liquidity just submit regular limit orders. If they want capital efficiency, they become JIT providers.

---

## Pseudocode: Complete JIT Flow

```rust
// ============ TYPES ============

struct Batch {
    id: BatchId,
    orders: Vec<Order>,
    seal_time: Timestamp,
}

struct JITBid {
    mm_id: MMId,
    commitment: Hash,
    orders: Option<Vec<Order>>,  // None until revealed
    fee_bid: Option<Decimal>,    // None until revealed
    nonce: Option<[u8; 32]>,     // None until revealed
}

struct JITFeeState {
    base_fee_rate: Decimal,
    target_jit_ratio: Decimal,
    history: VecDeque<BatchJITStats>,
}

// ============ MAIN FLOW ============

async fn process_batch(batch: Batch, fee_state: &mut JITFeeState) -> BatchResult {
    // 1. Compute base solution (no JIT)
    let base_solution = solve_base(&batch.orders);

    // 2. Compute patches from solvers
    let patches = collect_patches(&batch.orders, &base_solution).await;
    let selected_patches = select_patches(&patches);
    let patched_solution = apply_patches(&base_solution, &selected_patches);

    // 3. JIT commitment phase
    let commitments = collect_jit_commitments(JIT_COMMIT_WINDOW).await;

    // 4. JIT reveal phase
    let revealed_bids = collect_jit_reveals(&commitments, JIT_REVEAL_WINDOW).await;

    // 5. Validate and select JIT
    let valid_bids = revealed_bids
        .into_iter()
        .filter(|bid| validate_jit_bid(bid, &patched_solution, fee_state))
        .collect::<Vec<_>>();

    let selected_jit = select_jit_bids(&valid_bids, &patched_solution);

    // 6. Final solve
    let jit_orders: Vec<Order> = selected_jit
        .iter()
        .flat_map(|bid| bid.orders.clone().unwrap())
        .collect();

    let all_orders = [batch.orders.clone(), jit_orders].concat();
    let final_solution = solve_final(&all_orders, &selected_patches);

    // 7. Compute fees and rebates
    let fee_results = compute_jit_fees(&selected_jit, &patched_solution, &final_solution);
    let rebates = compute_rebates(&fee_results);

    // 8. Update fee state
    let stats = BatchJITStats {
        total_volume: final_solution.total_volume(),
        jit_volume: jit_orders.iter().map(|o| o.filled_volume(&final_solution)).sum(),
    };
    fee_state.update(&stats);

    // 9. Return result
    BatchResult {
        solution: final_solution,
        jit_fees: fee_results,
        rebates,
        patches_applied: selected_patches,
    }
}

// ============ JIT VALIDATION ============

fn validate_jit_bid(
    bid: &JITBid,
    base_solution: &Solution,
    fee_state: &JITFeeState
) -> bool {
    let orders = bid.orders.as_ref().unwrap();
    let fee_bid = bid.fee_bid.unwrap();

    // Verify commitment
    let expected_hash = hash(orders, fee_bid, bid.nonce.unwrap());
    if expected_hash != bid.commitment {
        return false;  // Invalid reveal
    }

    // Compute welfare delta
    let new_solution = solve_with_orders(base_solution, orders);
    let welfare_delta = new_solution.welfare - base_solution.welfare;

    // Must improve welfare
    if welfare_delta <= Decimal::ZERO {
        return false;
    }

    // Fee must meet minimum
    let required_fee = fee_state.base_fee_rate * welfare_delta;
    if fee_bid < required_fee {
        return false;
    }

    true
}

// ============ FEE UPDATES ============

impl JITFeeState {
    fn update(&mut self, stats: &BatchJITStats) {
        self.history.push_back(stats.clone());
        if self.history.len() > 100 {
            self.history.pop_front();
        }

        // Compute recent JIT ratio
        let recent_jit: Decimal = self.history.iter().map(|s| s.jit_volume).sum();
        let recent_total: Decimal = self.history.iter().map(|s| s.total_volume).sum();
        let actual_ratio = recent_jit / recent_total;

        // Adjust fee
        if actual_ratio > self.target_jit_ratio * Decimal::new(11, 1) {
            // >10% above target: increase fee
            self.base_fee_rate = (self.base_fee_rate * Decimal::new(1125, 3))
                .min(MAX_JIT_FEE_RATE);
        } else if actual_ratio < self.target_jit_ratio * Decimal::new(9, 1) {
            // >10% below target: decrease fee
            self.base_fee_rate = (self.base_fee_rate * Decimal::new(875, 3))
                .max(MIN_JIT_FEE_RATE);
        }
        // Otherwise: fee stays stable
    }
}

// ============ REBATE COMPUTATION ============

fn compute_rebates(fee_results: &[JITFeeResult]) -> Vec<Rebate> {
    let mut rebates = Vec::new();

    for result in fee_results {
        let rebate_pool = result.fee_paid * REBATE_SHARE;  // e.g., 70%

        // Find negatively affected users
        let negative_impacts: Vec<_> = result.affected_users
            .iter()
            .filter(|u| u.impact_amount < Decimal::ZERO)
            .collect();

        let total_negative: Decimal = negative_impacts
            .iter()
            .map(|u| u.impact_amount.abs())
            .sum();

        for user in negative_impacts {
            let share = user.impact_amount.abs() / total_negative;
            rebates.push(Rebate {
                user_id: user.user_id,
                amount: rebate_pool * share,
                reason: RebateReason::JITDisplacement,
            });
        }
    }

    rebates
}
```

---

## Summary: Mainline Design

| Aspect | Design Choice | Rationale |
|--------|---------------|-----------|
| JIT requirement | Must improve welfare | Prevents extractive JIT |
| Fee mechanism | EIP-1559 dynamic | Self-regulating, predictable |
| Fee level | 20% of welfare delta (adjusts) | Balance MM incentive vs user protection |
| Auction type | Blind (commit-reveal) | Prevents races and info games |
| Rebates | 70% of fee to affected users | Fairness to displaced |
| Flash quoting | Not separate, use JIT | JIT is strictly more flexible |
| Timing | After patches, before final solve | JIT sees best available state |

### Alternatives Noted

1. **Fixed fee instead of dynamic**: Simpler but less adaptive
2. **Open auction instead of blind**: Faster but enables gaming
3. **No rebates**: Simpler but less fair
4. **JIT before patches**: Less info for MMs, maybe less JIT quality
5. **Allow JIT on same markets as patches**: Complex conflict resolution

---

## V1 Design Decision (Final)

**See [jit-displacement-economics.md](./jit-displacement-economics.md) for full analysis.**

### Key Insight: "Rekt Passive LP" Is Not a Big Deal

Earlier analysis worried about passive LPs getting picked off. This concern is overblown:

- With excess demand, clearing price is pushed UP toward demand's limit
- Passive LP with stale $0.50 quote gets filled at $0.95+ (the UCP)
- UCP mechanics already protect passive LPs automatically

### What JIT Actually Does

1. **Adds volume** - fills more orders when there's demand/supply imbalance
2. **Allows informed flow** - JIT providers participate in price discovery
3. **For a cost** - JIT is taxed

### V1 Design

1. **JIT with displacement allowed** (not backrun-only)
   - Displacement just affects WHO fills, not the price (UCP)
   - Allows JIT to add liquidity freely

2. **JIT is taxed** (exact formula TBD)
   - Displacement portion: taxed
   - Backrun portion: possibly not taxed

3. **Semi-private orderbook**
   - Orderbook revealed anonymously to external JIT providers after batch pre-seals
   - JIT providers can see demand/supply imbalance
   - Privacy maintained until pre-seal
