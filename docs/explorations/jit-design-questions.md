# JIT Design: Critical Questions Exploration

This document explores three fundamental questions about the JIT liquidity design that must be resolved before implementation.

---

## Question 1: Should Matching Happen Before JIT?

### Current Design (from plan)

```
Orders Accumulate → Batch Seals → Base Solution Computed → JIT Window → Final Solution
```

JIT providers see the base solution and can then:
- **Backrun**: Fill remaining imbalances (unfilled demand/supply)
- **Displace**: Replace passive orders with better JIT liquidity

### Alternative: JIT Before Matching

```
Orders Accumulate → Batch Seals → JIT Window → Combined Matching
```

JIT providers see only the raw orderbook, submit liquidity, then everything matches together.

### Analysis

| Aspect | JIT After Matching | JIT Before Matching |
|--------|-------------------|---------------------|
| **Information to JIT** | Sees clearing prices + unfilled demand | Sees only raw orderbook |
| **JIT advantage** | Knows exact market state | Must guess clearing price |
| **Passive LP protection** | Passive fills first, JIT fills remainder | Competes equally |
| **Backrun clarity** | Clear what's "unfilled" | No clear backrun/displacement distinction |
| **Implementation** | Two-phase solve | Single combined solve |
| **JIT pricing** | Can price at/near clearing | Must predict clearing |

### The Core Tension

**JIT After Matching (current design):**
- Pro: JIT sees clearing price, can provide precisely calibrated liquidity
- Pro: Clear distinction between backrun (pure value-add) and displacement (needs justification)
- Con: JIT has information advantage over passive LPs
- Con: Two-phase solving is more complex

**JIT Before Matching:**
- Pro: Level playing field - JIT doesn't know more than passive LPs
- Pro: Simpler single-phase matching
- Con: JIT must guess clearing prices (may provide worse liquidity)
- Con: No clear backrun concept - everything competes equally
- Con: JIT may be less willing to participate without price certainty

### Recommendation

**Keep JIT after base matching** but recognize the information advantage is the feature, not a bug:

1. **JIT's value proposition** is providing liquidity with better information. If JIT had the same info as passive LPs, why would they bother? They'd just be passive LPs.

2. **The tax on displacement** compensates for the information advantage. JIT must create welfare improvement and pay for the privilege of late information.

3. **Backrun is genuinely harmless** - filling unfilled demand doesn't hurt anyone. This is clear only if we know what would have filled without JIT.

4. **UCP still protects passive LPs** - they get the fair clearing price regardless of whether JIT participates. JIT affects WHO fills, not the price passive LPs receive.

### Open Issue

If we're concerned about JIT's information advantage, we could:
- Reveal less information (e.g., only show imbalance direction, not magnitude)
- Add noise to clearing prices
- Shorter JIT window (less time to compute optimal response)

---

## Question 2: How Do Multiple JIT Providers Compete?

### Current Plan Gap

The plan mentions:
```rust
providers: Vec<Box<dyn JitProvider>>
```

But doesn't specify:
1. Do providers see each other's submissions?
2. How do we select among competing submissions?
3. What happens if submissions conflict?

### Competition Models

#### Model A: Blind Auction

```
1. All providers receive JitInput simultaneously
2. Each submits JitSubmission independently (can't see others)
3. Coordinator selects best combination of orders
4. Winners are notified, losers rejected
```

**Selection criteria** (in order):
1. Welfare improvement (primary)
2. Tax/fee willingness (secondary)
3. Timestamp (tiebreaker)

**Pros:**
- Prevents gaming (can't penny-jump)
- Encourages honest bidding
- Fair competition

**Cons:**
- Providers may overbid (winner's curse)
- Can't iterate to find optimal solution

#### Model B: Sequential Auction

```
1. Provider 1 submits, gets tentative acceptance
2. Provider 2 sees Provider 1's fill, submits better or passes
3. ...repeat...
4. Final provider has last look
```

**Cons:**
- Last-mover advantage
- Race conditions
- Unfair to early submitters

#### Model C: MWIS Combination (Recommended)

```
1. All providers submit independently (blind to each other)
2. All submissions validated
3. Build conflict graph:
   - Nodes = valid JIT orders
   - Edges = orders that conflict (can't both fill)
4. Solve MWIS (Maximum Weight Independent Set)
   - Weight = welfare_improvement - tax_to_pay
5. Selected orders execute, rest rejected
```

This is the same approach used for combining solver solutions - proven to work.

**Conflict types:**
- Same liquidity: Two JIT orders filling same demand
- Cross-market: Orders affecting same user/position
- Welfare dependency: Order A's welfare depends on Order B not existing

**Pros:**
- Optimal combination of multiple providers
- No gaming between providers
- Reuses existing MWIS infrastructure

**Cons:**
- More complex implementation
- May need more sophisticated conflict detection

### Recommendation

**Use MWIS combination (Model C)** for selecting among competing JIT submissions:

```rust
impl JitCoordinator {
    fn select_jit_orders(&self, submissions: Vec<ValidatedJit>) -> Vec<ValidatedJitOrder> {
        // 1. Flatten all orders from all providers
        let all_orders: Vec<_> = submissions.iter()
            .flat_map(|s| s.orders.iter())
            .collect();

        // 2. Build conflict graph
        let conflicts = self.build_jit_conflict_graph(&all_orders);

        // 3. Solve MWIS with welfare as weight
        let selected = mwis_solve(&conflicts, |order| order.welfare_improvement);

        selected
    }
}
```

---

## Question 3: What Does "20% Tax" Actually Mean?

### The Confusion

The plan says "20% tax" but doesn't clearly specify 20% of what:
- 20% of price? (No - prices can be near zero or very high)
- 20% of JIT volume? (No - doesn't relate to welfare)
- 20% of welfare improvement? (Intended, but needs clarity)

### Clarifying Welfare Improvement

**Welfare** in this system = sum of (order's value to user - execution cost)

For a JIT order that improves welfare:
```
welfare_improvement = welfare_with_jit - welfare_without_jit
```

This captures:
- Additional fills that wouldn't have happened (backrun)
- Better prices for users (displacement effect)
- More efficient allocation

### Tax Calculation Examples

**Example 1: Pure Backrun**
```
Base solution:
  - User A: Buy 100 @ $0.60 → Fills 80 (supply shortage)

JIT provides: Sell 20 @ $0.58

After JIT:
  - User A: Fills full 100
  - Welfare improvement: User A gets 20 more fills at favorable price
  - Say welfare_delta = $4.00 (20 units × $0.20 surplus each)

Tax = 20% × $4.00 = $0.80 (paid by JIT provider)
JIT keeps: $4.00 - $0.80 = $3.20
```

Wait - this is wrong. The JIT provider doesn't "capture" the welfare improvement. Let me reconsider.

**Welfare accounting:**
- User welfare = value_to_user - price_paid
- JIT welfare = price_received - cost_to_provide

If JIT sells at $0.58 and their cost is $0.50:
- JIT surplus = ($0.58 - $0.50) × 20 = $1.60
- User A surplus increase = (value - $0.58) × 20

**The tax should be on JIT's profit**, not on total welfare improvement.

### Revised Understanding

```
JIT_profit = revenue - cost
Tax = 20% × JIT_profit
JIT_net = JIT_profit - Tax = 80% × JIT_profit
```

For displacement:
```
JIT_profit = revenue - cost
Displaced_LP_loss = what_they_would_have_earned
Rebate_pool = Tax
Rebates = distributed proportionally to displaced LPs
```

### Is 20% Prohibitively High?

Let's do the math:

**Scenario: JIT provides displacement liquidity**
- JIT cost to provide: $0.50 per unit
- Clearing price: $0.55 per unit
- Volume: 100 units
- Gross profit: ($0.55 - $0.50) × 100 = $5.00

With 20% tax on profit:
- Tax: $1.00
- Net profit: $4.00

**Question: Is $4.00 profit on $50 capital worth it?**

$4.00 / $50.00 = 8% return per batch

If there are 10 batches per day: 80% daily return (!)

This seems extremely profitable, even with 20% tax.

**But wait** - the tax is on "welfare improvement", not JIT profit specifically. These may differ.

### The Real Issue: What Is Being Taxed?

Looking at the original design doc:
```
Fee = base_fee × welfare_delta
```

Where `welfare_delta = welfare_with_jit - welfare_without_jit`

This is **total welfare improvement**, which includes:
- JIT's profit
- User's improved fills
- Better price discovery

If JIT's profit is $5 but total welfare improvement is $20, tax is:
- Tax = 20% × $20 = $4
- JIT keeps: $5 - $4 = $1 (only 20% of their gross!)

This IS potentially prohibitive.

### Recommendation: Tax JIT's Share of Welfare

```rust
struct JitTaxCalculation {
    total_welfare_improvement: i64,
    jit_welfare_share: i64,      // What JIT captures
    user_welfare_share: i64,     // What users gain

    // Tax only JIT's share
    tax_rate: f64,               // e.g., 0.20
    tax_amount: i64,             // tax_rate × jit_welfare_share
}
```

Or simpler: **Tax JIT's gross profit directly**

```rust
fn calculate_jit_tax(jit_order: &JitOrder, fill_price: Nanos, jit_cost: Nanos) -> Nanos {
    let gross_profit = (fill_price - jit_cost) * fill_qty;
    let tax = gross_profit * TAX_RATE;  // 20% of profit, not welfare
    tax
}
```

**Problem**: We don't know JIT's true cost. They could claim cost = fill_price to avoid tax.

### Alternative: Percentage of Fill Value

```rust
fn calculate_jit_tax(fill_price: Nanos, fill_qty: Qty) -> Nanos {
    let fill_value = fill_price * fill_qty;
    fill_value * TAX_RATE  // e.g., 0.5% of notional
}
```

This is simpler and harder to game, but doesn't scale with profitability.

### Final Recommendation

**For bootstrap/V1:**
1. Use simple fee: `fee = fill_value × flat_rate` (e.g., 0.5% of notional)
2. Backrun: 0% fee (pure value add)
3. Displacement: 0.5% fee (or whatever makes economic sense)

**For V2 (with competition):**
- Dynamic fee (EIP-1559 style) based on JIT utilization
- Let competition drive fees to equilibrium

---

## Summary: Recommended Decisions

| Question | Recommendation |
|----------|---------------|
| JIT timing | After base matching (JIT sees clearing prices) |
| Provider competition | MWIS combination of all valid submissions |
| Tax basis | Notional value for V1; dynamic for V2 |
| Backrun tax | Zero (pure value add) |
| Displacement tax | Small % of notional (~0.5%) |

---

## Implementation Impact

These decisions affect the implementation:

1. **JitInput must include clearing prices** from base solution
2. **JitCoordinator needs MWIS solver** for combining submissions
3. **Tax calculation is simple** for V1 (flat rate on notional)
4. **Backrun vs Displacement classification** remains important (different tax rates)

---

## Open Questions for Further Exploration

1. **How do we know JIT's true cost?** Without this, profit-based taxes are gameable.
2. **Should displaced LPs get rebates?** From JIT fees or protocol treasury?
3. **What's the optimal JIT window duration?** Tradeoff between quality and latency.
4. **Should there be JIT position limits?** To prevent one provider dominating.
