# JIT Liquidity Design

## Overview

JIT (Just-In-Time) liquidity allows market makers to provide liquidity AFTER seeing the sealed batch but BEFORE final execution. This addresses the capital efficiency problem: MMs don't need to lock capital in limit orders that may never fill.

## How FBA Changes Adverse Selection

### Traditional CLOB Problem
- MM quotes bid $0.48, ask $0.52
- Informed trader hits ask at $0.52
- Price immediately moves to $0.60
- MM stuck holding at $0.52 when market is $0.60 ("picked off")

### FBA Solution
- MM quotes bid $0.48, ask $0.52
- Informed trader submits buy
- Batch clears at $0.55 (informed flow moves the price UP)
- **MM sells at $0.55, not $0.52** - gets the fair price!

**Key insight**: FBA protects MMs because everyone gets the clearing price. The "adverse selection" in FBA is about being on the wrong SIDE, not getting a bad PRICE.

---

## The UCP Protection Mechanism

### Why "Rekt Passive LP" Isn't a Big Deal

Consider this scenario:
- Passive LP: Sell 100 @ $0.50 (stale quote)
- News breaks: true value shoots to $1.00
- Informed traders: Buy 10,000 @ $0.95-$0.99

**What actually happens (UCP mechanics)**:
- Supply: 100 units at any price >= $0.50
- Demand: 10,000 units at any price <= $0.95
- Excess demand at all prices -> demand competes for scarce supply
- Clearing price pushed UP toward demand's limit
- **Result: Passive LP sells at $0.95-$0.99, not their stale $0.50!**

The passive LP's limit price ($0.50) is just their MINIMUM - they receive the Uniform Clearing Price (UCP), which is determined by how aggressively informed traders bid.

### Volume-Weighted Clearing Price Rule

When supply/demand curves overlap:
```
B = total buy volume (at or above clearing range)
S = total sell volume (at or below clearing range)
P_bid = best bid (highest buy price)
P_ask = best ask (lowest sell price)

weight = B / (B + S)
Clearing price = P_ask + weight × (P_bid - P_ask)
```

Properties:
- If B >> S: price -> P_bid (demand pulls price up)
- If S >> B: price -> P_ask (supply pulls price down)
- If B = S: price = midpoint

**Example**: Sell 100 @ $0.50, Buy 10,000 @ $0.95
- weight = 10,000 / 10,100 ≈ 0.99
- price = $0.50 + 0.99 × $0.45 = $0.9455

---

## JIT Mechanism Design

### What JIT Actually Does

1. **Adds volume** - Fills more orders when there's demand/supply imbalance
2. **Price discovery** - JIT providers participate in finding fair prices
3. **Taxed participation** - JIT pays fees for information advantage

### Design: JIT with Displacement

**Rule**: JIT can participate in the full batch, including "displacement" of passive orders.

**Rationale**:
1. UCP protects passive LPs (they get the fair clearing price)
2. JIT helps anchor prices correctly during volatile flows
3. "Displacement" in UCP just affects WHO fills, not price extraction
4. Backrun-only would keep fair prices out, actually hurting LPs

### JIT Flow

```
1. Batch seals with user orders
2. Base solution computed (single-market, no JIT)
3. Orderbook revealed (anonymously) to JIT providers
4. JIT window opens (~200ms)
5. JIT providers submit orders + fee bids
6. JIT window closes
7. JIT orders evaluated for welfare improvement
8. Accepted JIT orders added to solution
9. Final solution computed
10. Batch clears
```

---

## Fee Structure

### Dynamic Fee (EIP-1559 Style)

```rust
struct JITFeeState {
    base_fee: Decimal,           // Current base fee rate
    target_jit_ratio: Decimal,   // Target: JIT volume / total volume
}

impl JITFeeState {
    fn update(&mut self, batch_stats: &BatchStats) {
        let actual_ratio = batch_stats.jit_volume / batch_stats.total_volume;

        if actual_ratio > self.target_jit_ratio {
            // Too much JIT, increase fee
            self.base_fee *= 1.125;  // +12.5%
        } else {
            // Room for more JIT, decrease fee
            self.base_fee *= 0.875;  // -12.5%
        }

        self.base_fee = self.base_fee.clamp(MIN_FEE, MAX_FEE);
    }

    fn required_fee(&self, welfare_delta: Decimal) -> Decimal {
        self.base_fee * welfare_delta
    }
}
```

**Parameters**:
- `target_jit_ratio`: 0.30 (30% of volume from JIT)
- `MIN_FEE`: 0.05 (5% of welfare delta)
- `MAX_FEE`: 0.50 (50% of welfare delta)
- Initial `base_fee`: 0.20 (20% of welfare delta)

### Why Dynamic?

- **Self-regulating**: Fee finds equilibrium
- **Predictable**: MMs can estimate required fee
- **Prevents JIT domination**: If too much JIT, fee rises
- **Allows JIT growth**: If too little JIT, fee falls

### Displacement vs Backrun Taxation

| JIT Type | Definition | Tax |
|----------|------------|-----|
| Backrun | Fills unfilled demand only | Lower/none |
| Displacement | Fills orders that passive LPs would have | Higher |

The distinction is TBD - may have uniform taxation for simplicity.

---

## Welfare Requirement

JIT must improve total welfare to be accepted:

```rust
fn evaluate_jit(base: &Solution, jit_orders: &[Order]) -> Option<JITResult> {
    let new_solution = solve_with_orders(base, jit_orders);
    let welfare_delta = new_solution.welfare - base.welfare;

    // Must improve welfare
    if welfare_delta <= 0 {
        return None;
    }

    // Must meet minimum threshold (anti-spam)
    if welfare_delta < MIN_IMPROVEMENT {
        return None;
    }

    Some(JITResult {
        welfare_delta,
        orders: jit_orders.to_vec(),
    })
}
```

---

## Rebate Distribution

When JIT affects existing users, rebates compensate negatively impacted parties:

```rust
enum Impact {
    PriceImprovement { amount: Decimal },  // User got better price
    NegativeImpact { amount: Decimal },    // User got worse fill
    Displaced { would_have_received: Decimal },
}

fn distribute_rebates(fee_paid: Decimal, affected: &[AffectedUser]) {
    let protocol_share = fee_paid * 0.30;  // 30% to protocol
    let rebate_pool = fee_paid * 0.70;     // 70% to rebates

    let total_negative: Decimal = affected
        .iter()
        .filter_map(|u| match &u.impact {
            Impact::NegativeImpact { amount } => Some(*amount),
            Impact::Displaced { amount } => Some(*amount),
            _ => None,
        })
        .sum();

    for user in affected {
        if let Some(negative_amount) = user.negative_impact() {
            let rebate = rebate_pool * (negative_amount / total_negative);
            credit_user(user.id, rebate);
        }
    }
}
```

---

## Anti-Gaming Measures

### Blind Auction (Optional)

To prevent JIT races:

```
Phase 1: Commitment (blind)
  - MMs submit hash(orders || fee || nonce)
  - Nobody sees actual bids

Phase 2: Reveal
  - MMs reveal (orders, fee, nonce)
  - Verify hash matches commitment

Phase 3: Selection
  - Evaluate all valid JIT bids
  - Select best combination
```

### Anti-Penny-Jumping

**Problem**: MM sees Bob's Sell @ $0.50, submits JIT Sell @ $0.499999.

**Solution**: Asymmetric fees
- External orders: 0 bps fee (or rebate)
- JIT orders: 5+ bps fee

Creates a "moat" - MM must improve price by >5 bps to profitably displace.

**Tie-breaker**: If price(JIT) == price(External), External gets 100% priority.

---

## Integration with Solving Pipeline

```
┌─────────────────────────────────────────────────────────┐
│                    Batch Solving Pipeline                │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  1. User orders sealed                                   │
│          ↓                                               │
│  2. Base solution (single-market, no JIT)               │
│          ↓                                               │
│  3. Cross-market patches from solvers                    │
│          ↓                                               │
│  4. JIT window: MMs see patched solution                │
│          ↓                                               │
│  5. JIT orders submitted and evaluated                   │
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

JIT providers see the **patched** solution, not just the base. This gives them the best available price information.

---

## V1 Design Summary

| Aspect | Design Choice | Rationale |
|--------|---------------|-----------|
| Displacement | Allowed | UCP protects passive LPs anyway |
| Fee mechanism | EIP-1559 dynamic | Self-regulating |
| Fee level | ~20% of welfare delta | Balance MM incentive vs protection |
| Rebates | 70% of fee to affected users | Fairness |
| Auction type | TBD (open or blind) | Blind prevents races |
| Timing | After patches | JIT sees best state |

---

## Open Questions

1. **Exact tax formula** - How to balance MM incentives vs extraction prevention
2. **Backrun tax exemption** - Should pure liquidity provision be tax-free?
3. **Cross-market JIT** - How does JIT interact with patches?
4. **Tax distribution** - Protocol revenue vs burned vs distributed

---

## Implementation Status

The `jit-study` crate contains research simulations for JIT mechanisms. Production JIT is not yet implemented in the main solving pipeline.

See `jit-study/src/` for experimental code exploring:
- Welfare impact of JIT
- Fee optimization
- Displacement effects
