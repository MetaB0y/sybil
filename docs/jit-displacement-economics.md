# JIT Displacement Economics: Deep Dive

## The Core Question

When JIT liquidity arrives, should it be allowed to **displace** existing passive orders?

**Displacement** = MM's JIT order fills instead of (some of) an existing passive order

```
Before JIT:
  Alice: Buy 100 @ 0.60
  Bob: Sell 100 @ 0.40
  Clear at 0.50, both fill 100

With JIT (MM sells 50 @ 0.45):
  Clear at 0.475 (more supply → lower price)
  Bob fills 75 (displaced by 25)
  MM fills 25
  Alice gets better price
```

---

## Option Comparison

### Option A: Full Displacement (Welfare Only)

**Rule**: JIT can displace. Only requirement: total welfare improves.

**Pros**:
- Maximum efficiency
- Best prices for takers
- Simple rule

**Cons**:
- "Normal market behavior" — but we're trying to be better
- Passive LPs get squeezed when MMs want their flow
- Discourages passive liquidity provision
- PR: "Your order gets front-run if someone pays more"

**Who wins**: Takers (better prices), MMs (cherry-pick flow)
**Who loses**: Passive LPs (displaced), Protocol reputation

---

### Option B: Backrun Only

**Rule**: JIT can only fill orders that would otherwise go UNFILLED.

```
Before JIT:
  Alice: Buy 100 @ 0.60
  Bob: Sell 50 @ 0.40
  Clear at 0.50, Alice fills 50, Bob fills 50

With JIT (MM sells 50 @ 0.45):
  Alice fills 100 (50 from Bob, 50 from MM)
  Bob fills 50 (unchanged)
  MM fills 50
  Alice gets more filled, same price
```

**Pros**:
- Zero displacement, pure additive
- "Fair" — existing orders always respected
- Flash-liquidity properties (MM provides when needed, no capital locked)
- Good PR: "We never front-run your order"

**Cons**:
- Less efficient (can't replace bad passive liquidity with better)
- MM can only fill excess demand, not improve prices
- What about when passive liquidity is overpriced?

**Who wins**: Passive LPs (protected), Takers (more fills)
**Who loses**: Efficiency

---

### Option C: Priority Tiers

**Rule**: Passive orders have priority within each price level. JIT fills remainder.

Same as Option B in practice. Slightly more complex rule for same outcome.

---

### Option D: Displacement with Compensation

**Rule**: JIT can displace, but must compensate displaced party.

**Non-messy version?**

The mess comes from calculating "fair" compensation. Let's try to simplify:

**Approach 1: Fixed displacement fee**
```
If JIT displaces X shares from Bob:
  MM pays: X × DISPLACEMENT_FEE_RATE × clearing_price
  Bob receives: that amount
```

Problem: What's the right DISPLACEMENT_FEE_RATE? Too low = displacement is free. Too high = no JIT.

**Approach 2: Bid for displacement**
```
MM states: "I'll pay Y to displace Bob"
If Y > threshold, displacement allowed
Bob receives Y
```

Problem: What's threshold? How does MM know what to bid?

**Approach 3: Proportional to welfare delta**
```
welfare_delta = welfare_with_jit - welfare_without_jit
mm_share = welfare_delta × 0.5
bob_rebate = welfare_delta × 0.3
protocol_fee = welfare_delta × 0.2
```

This is... actually not that messy?

- MM gets paid for improving welfare
- Bob gets compensated for being displaced
- Protocol takes cut
- All calculated from welfare numbers we already compute

**Approach 4: Bob's surplus protection**
```
Bob's surplus without JIT: (clearing_price - bob_limit_price) × bob_fill_qty
Bob's surplus with JIT: (new_clearing_price - bob_limit_price) × new_bob_fill_qty

If Bob's surplus decreased:
  rebate = surplus_decrease
  MM pays rebate
```

This ensures: Bob is never worse off in $ terms (he might fill less but at better price + rebate)

---

## Economic Analysis: What Actually Matters?

### For Passive LPs (like Bob)

What does Bob care about?

1. **Getting filled** — wants his order to execute
2. **Good price** — wants best possible execution price
3. **Predictability** — wants to know what will happen

Option A: Unpredictable. Might get displaced, might not.
Option B: Predictable. Always fills if matched.
Option D: Predictable with compensation. "You might be displaced but you'll be made whole."

### For MMs

What do MMs want from JIT?

1. **Rebalancing** — got too much inventory on one side, need to offload
2. **Toxic flow avoidance** — don't want to be picked off by informed traders
3. **Profit** — capture spread when providing liquidity

Option A: Maximum freedom. Can rebalance by displacing.
Option B: Can only fill excess. Limited rebalancing.
Option D: Can rebalance if willing to pay.

### Wait — Is "Rebalancing" Actually Important?

In traditional markets, MMs accumulate inventory from random flow, then rebalance.

In FBA:
- MMs don't have standing quotes that get hit randomly
- JIT means MM *chooses* when to provide liquidity
- MM sees the batch, decides if they want to participate
- No forced accumulation → less rebalancing need?

Maybe rebalancing is overrated for JIT model. MM can simply... not provide liquidity when it would create bad inventory.

### The "Toxic Flow" Question

What is toxic flow in FBA?

Traditional: Informed trader hits your quote, price moves against you, you lose.

FBA:
- Informed trader submits order
- Batch clears at uniform price
- MM (if passive) gets same price as everyone

The "toxic" part: MM provides liquidity at price X, but true value was Y > X. MM loses Y - X per share.

**JIT changes this**: MM sees the batch, can estimate if flow is toxic based on:
- Size (huge order = maybe informed?)
- Cross-market orders (complex position = sophisticated trader?)
- Recent news (if Trump just tweeted, all TRUMP orders are suspect)

JIT lets MM *avoid* toxic flow entirely by not participating.

So Option B (backrun only) is actually fine for MMs because:
- They don't have to provide liquidity at all
- They choose when to participate
- If flow looks toxic, they skip that batch

---

## Reframing: What Problem Are We Solving?

Original framing: "Should JIT displace passive orders?"

Better framing: "What's the optimal mix of passive and JIT liquidity?"

### Scenario: Healthy Market

- Many passive LPs
- Tight spreads
- JIT adds marginal improvement

Here: Backrun-only is fine. JIT fills the gaps.

### Scenario: Thin Market

- Few passive LPs
- Wide spreads
- JIT could dramatically improve prices

Here: Allowing displacement helps takers get better prices.

But wait — if spreads are wide, there's probably unfilled demand. JIT can backrun that unfilled demand without displacing anyone.

### Scenario: Bad Passive Liquidity

- Passive LP is pricing poorly (stale quote, wrong about market)
- JIT has better price
- Backrun-only means bad passive LP fills, good JIT doesn't

This is the real case for displacement. Do we care?

**Argument for caring**: Taker deserves best price
**Argument against**: Passive LP took risk of posting order, should be rewarded

---

## Proposal: Tiered Approach

**V1: Backrun Only**

Simple, fair, good PR. MMs can provide liquidity for unfilled demand.

```rust
fn is_valid_jit_order(jit: Order, base_solution: Solution) -> bool {
    // JIT can only fill demand that's currently unfilled
    let unfilled = get_unfilled_demand(&base_solution, jit.market, jit.side.opposite());
    jit.quantity <= unfilled
}
```

**V2: Displacement with Surplus Protection**

If market matures and efficiency matters more:

```rust
fn process_jit_with_protection(jit: Order, base: Solution) -> (Solution, Vec<Rebate>) {
    let new_solution = apply_jit(&base, &jit);

    let mut rebates = vec![];
    for order in affected_passive_orders(&base, &new_solution) {
        let old_surplus = calculate_surplus(&base, &order);
        let new_surplus = calculate_surplus(&new_solution, &order);

        if new_surplus < old_surplus {
            let rebate = old_surplus - new_surplus;
            rebates.push(Rebate { to: order.user, amount: rebate });
        }
    }

    // JIT provider pays for rebates
    let total_rebate: Decimal = rebates.iter().map(|r| r.amount).sum();
    charge_jit_provider(jit.provider, total_rebate);

    (new_solution, rebates)
}
```

This is Option D but non-messy because:
- Clear rule: "You can displace, but displaced party keeps their surplus"
- No arbitrary fees or thresholds
- Self-balancing: expensive to displace someone who would've profited a lot

---

## Fee Discussion

Separate from displacement: what's the fee for JIT?

### Purpose of Fee

1. **Spam prevention** — don't submit garbage patches
2. **Protocol revenue** — capture some value
3. **Balance incentives** — prevent JIT from dominating

### Options

**Fixed minimum**: e.g., $0.01 per affected market
- Simple
- Might be too high for thin markets, too low for active

**Percentage of welfare**: e.g., 20% of welfare improvement
- Scales with value created
- Self-balancing

**EIP-1559 style**: Base fee adjusts based on JIT usage
- Responds to demand
- Complex, slow to adapt

**No explicit fee, just compete**:
- JIT must offer better prices to be selected
- "Fee" is implicit in price improvement
- Simple but might allow spam

### Recommendation

**Welfare percentage** seems cleanest:

```rust
const JIT_FEE_RATE: Decimal = 0.20; // 20%

fn process_jit(jit: JitSubmission, base: Solution) -> Option<Solution> {
    let new_solution = apply_jit(&base, &jit);
    let welfare_delta = new_solution.welfare - base.welfare;

    // Must improve welfare
    if welfare_delta <= 0 {
        return None;
    }

    // Must improve by at least MIN_IMPROVEMENT (anti-spam)
    if welfare_delta < MIN_IMPROVEMENT {
        return None;
    }

    // Charge fee
    let fee = welfare_delta * JIT_FEE_RATE;
    charge_provider(jit.provider, fee);

    Some(new_solution)
}
```

Why not EIP-1559?
- JIT demand is highly variable (news → spike, quiet period → nothing)
- Slow adaptation doesn't match fast market dynamics
- Fixed percentage is simpler and still aligns incentives

---

## Final Design Decision (Updated)

### Key Insight: UCP Changes Everything

The earlier analysis missed a crucial point about **Prediction Markets + Uniform Clearing Price (UCP)**:

In PMs, when news breaks (e.g., "Biden drops out"), there's a **liquidity vacuum**:
- Passive LPs have stale quotes (Sell @ $0.40)
- True value is now $0.80
- Informed traders pile in with bids at $0.85

**Without JIT in price formation (backrun-only):**
- Clearing price determined by stale LP vs informed demand
- Price might clear at $0.50-0.60
- Passive LP sells at $0.60 when asset worth $0.80 → **rekt**

**With JIT displacement allowed:**
- JIT provides liquidity at fair price ($0.80)
- Clearing price anchors at $0.80
- Passive LP (who offered $0.40) gets filled at $0.80 → **saved!**

The "displacement" fear was backwards:
- During news events: JIT can't displace passive LPs anyway (passive has cheaper price, fills first)
- During normal trading: JIT displacement = price improvement for traders
- With UCP: everyone gets same price, displacement just affects WHO fills, not AT WHAT PRICE

### V1 Design: JIT with Displacement

**Rule**: JIT can participate in full batch, including displacement.

**Rationale**:
1. UCP protects passive LPs (they get the anchored fair price)
2. JIT anchors prices correctly during violent PM flows
3. Backrun-only would actually hurt LPs by keeping fair prices out
4. "Displacement" in UCP just means who fills, not price extraction

### Fee Structure

**JIT is taxed** (exact formula TBD):
- Displacement portion: taxed (MM profits from taking flow)
- Backrun portion: possibly not taxed (pure liquidity provision)

The tax prevents excessive extraction while allowing JIT to improve price discovery.

### V1 Implementation: Semi-Private Book

For V1:
- Orderbook is **semi-private**
- After batch is **pre-sealed**, book is anonymously revealed to external JIT providers
- JIT providers submit orders based on revealed state
- Final batch includes JIT orders

This allows:
- JIT to see demand/supply imbalance
- JIT to provide liquidity at fair prices
- Price anchoring during news events
- Privacy until pre-seal (users don't know what others submitted)

---

## Open Questions

1. **Exact tax formula TBD** - needs to balance MM incentive vs extraction prevention

2. **Should backrun be tax-exempt?** Pure liquidity provision (filling unfilled demand) may deserve lower/no tax

3. **How does this interact with cross-market patches?** A patch might include JIT on one market and cross-market fill on another

4. **Tax distribution** - Protocol revenue vs burned vs distributed to affected users
