# JIT Competition Mechanisms: Deep Analysis

This document rigorously analyzes different approaches for handling competition between multiple JIT providers.

---

## The Core Question

When multiple JIT providers submit orders for the same batch, how do we select which orders to accept?

Two main approaches:
1. **Auction**: Pick the single best submission (like mev-boost)
2. **MWIS**: Combine compatible orders from multiple providers

---

## Understanding the Problem Structure

### What Is Being Selected?

| System | Submission Type | Selection | Combinable? |
|--------|----------------|-----------|-------------|
| **mev-boost** | Complete block | Pick 1 winner | No - blocks are mutually exclusive |
| **JIT (naive)** | Complete solution | Pick 1 winner | No - if treated as complete |
| **JIT (orders)** | Individual orders | Pick subset | Yes - orders may be complementary |

**Key insight**: JIT providers submit *orders*, not complete solutions. This fundamentally differs from mev-boost where builders submit complete blocks.

### When Do JIT Orders Conflict?

In a single market with unfilled demand D:

```
Unfilled demand: 100 units buy @ ≤$0.60

Provider A submits: Sell 40 @ $0.58
Provider B submits: Sell 80 @ $0.57
Provider C submits: Sell 60 @ $0.59
```

**Conflict analysis:**
- A alone: fills 40/100
- B alone: fills 80/100
- C alone: fills 60/100
- A+B: 40+80=120 > 100 → **partial conflict**
- A+C: 40+60=100 → **no conflict** (exactly fills demand)
- B+C: 80+60=140 > 100 → **partial conflict**
- A+B+C: 180 > 100 → **conflict**

### Conflict Types

1. **Full conflict**: Orders are mutually exclusive (can only pick one)
2. **Partial conflict**: Orders exceed demand but could partially combine
3. **No conflict**: Orders are complementary (different markets, or sum ≤ demand)

---

## Mechanism 1: Simple Auction (mev-boost style)

### How It Works

```
1. All providers submit JitSubmission (batch of orders)
2. Score each submission: score = welfare_improvement(submission)
3. Pick highest-scoring submission
4. Winner takes all, losers get nothing
```

### Pros
- **Simplicity**: Easy to implement and reason about
- **Predictability**: Providers know they either win or lose
- **No partial fills**: Each provider's orders treated as atomic bundle
- **Proven model**: Works well for mev-boost

### Cons
- **Misses combinations**: In example above, auction picks B (80 units) but misses A+C combo (100 units, better!)
- **Winner-take-all dynamics**: May discourage smaller providers
- **Underutilizes liquidity**: If winner provides 80/100, leaves 20 unfilled even though others could fill

### When Auction Works Well
- Providers submit comprehensive solutions
- Solutions are genuinely mutually exclusive
- Single dominant provider is acceptable

---

## Mechanism 2: MWIS (Maximum Weight Independent Set)

### How It Works

```
1. All providers submit orders (not necessarily bundles)
2. Build conflict graph:
   - Node per order
   - Edge between orders that conflict
3. Weight each node by welfare_improvement
4. Solve MWIS: find max-weight set of non-conflicting orders
5. Accept all orders in the independent set
```

### Conflict Graph for Example

```
Unfilled: 100 units

A: Sell 40    B: Sell 80    C: Sell 60
     \          /\            /
      \        /  \          /
       \      /    \        /
        (conflict) (conflict)

A+C = 100 ✓ (no edge between A and C)
A+B = 120 ✗ (edge: can't both fully fill)
B+C = 140 ✗ (edge)
```

**But wait** - this depends on how we define conflict.

### The Partial Fill Problem

MWIS assumes binary conflicts (orders either conflict or don't). But JIT orders can partially combine:

```
Demand: 100 units
A: Sell 40
B: Sell 80

Full A (40) + Partial B (60) = 100 ✓
```

This isn't naturally expressible in MWIS without order splitting.

### MWIS Variants

**Variant A: Atomic orders (no splitting)**
- Orders fill completely or not at all
- Conflict = sum exceeds demand
- Standard MWIS applies

**Variant B: Allow partial fills**
- Orders can fill partially
- No binary conflict concept
- Need different formulation (knapsack-like)

**Variant C: Transform to atomic**
- Split orders into smaller atomic units
- Apply MWIS to units
- More nodes, same algorithm

### Pros
- **Optimal combination**: Finds best mix of compatible orders
- **Multi-provider friendly**: Multiple providers can win
- **Maximizes fill**: Uses all available liquidity optimally

### Cons
- **More complex**: Harder to implement and verify
- **Partial fill handling**: Unclear without order splitting
- **Provider UX**: Providers don't know if they'll win or partially win
- **May not fit problem**: If orders are naturally bundles, MWIS is overkill

---

## Mechanism 3: Price-Priority Order Book

### How It Works

```
1. Providers submit individual orders (not bundles)
2. Build virtual order book with all JIT orders
3. Match against unfilled demand using price priority
4. Pro-rata at same price level
```

### Example

```
Unfilled demand: 100 units buy @ ≤$0.60

JIT supply:
  Provider B: Sell 80 @ $0.57 (best price)
  Provider A: Sell 40 @ $0.58
  Provider C: Sell 60 @ $0.59

Matching:
  1. Fill 80 from B @ $0.57
  2. Fill 20 from A @ $0.58 (partial)
  3. C not reached (demand satisfied)

Result: B gets 80, A gets 20, C gets 0
```

### Pros
- **Natural for markets**: Standard exchange mechanics
- **Handles partial fills**: Built-in
- **Price competition**: Best price wins
- **Simple to understand**: MMs know this model

### Cons
- **No bundle expression**: Can't say "I want all-or-nothing on my 40 units"
- **Price manipulation**: Could game with penny improvements
- **Doesn't capture welfare**: Optimizes for price, not welfare

---

## Mechanism 4: Hybrid Auction

### How It Works

```
1. Providers submit bundles (orders + bid)
2. Evaluate each bundle's welfare improvement
3. Sort by welfare/bid ratio (value per dollar of bid)
4. Greedily accept bundles until demand filled
5. Partial fill last bundle if needed
```

This is like a multi-unit auction with welfare as the objective.

### Pros
- **Bundle-aware**: Respects provider bundling preferences
- **Competitive**: Encourages efficient bidding
- **Partial fills**: Handled at bundle boundaries

### Cons
- **Greedy vs optimal**: May miss better combinations
- **Bid gaming**: Complex bidding dynamics

---

## Analysis: Which Mechanism When?

### Decision Tree

```
Are JIT orders naturally bundled (all-or-nothing)?
├─ YES → Are bundles mutually exclusive?
│        ├─ YES → Simple Auction (mev-boost style)
│        └─ NO  → MWIS on bundles
└─ NO  → Are orders for single market?
         ├─ YES → Price-Priority Order Book
         └─ NO  → MWIS on orders
```

### For Single-Market JIT

Since JIT is single-market:

**If orders are atomic (all-or-nothing)**:
- Providers submit "I'll sell 100 units, take it or leave it"
- Use **Simple Auction** or **MWIS** depending on combination potential

**If orders are partial-fillable**:
- Providers submit limit orders
- Use **Price-Priority Order Book** - it's the natural fit

### Recommendation for V1

**Price-Priority Order Book** because:
1. Single-market JIT = standard order matching
2. Partial fills are natural
3. MMs understand this model
4. Simpler than MWIS
5. Price competition is fair and transparent

```rust
fn select_jit_orders(
    unfilled: &UnfilledDemand,
    jit_orders: Vec<JitOrder>,
) -> Vec<JitFill> {
    // Sort by price (best first)
    let mut asks = jit_orders.iter().filter(|o| o.side == Sell);
    asks.sort_by_key(|o| o.price);  // Ascending for sells

    let mut fills = vec![];
    let mut remaining = unfilled.buy_qty;

    for order in asks {
        if remaining == 0 { break; }
        let fill_qty = min(order.quantity, remaining);
        fills.push(JitFill { order, fill_qty });
        remaining -= fill_qty;
    }

    fills
}
```

---

## WASM-in-TEE Considerations

When JIT providers are WASM blobs in TEE:

| Concern | Auction | MWIS | Price-Priority |
|---------|---------|------|----------------|
| **Determinism** | ✓ | ✓ | ✓ |
| **Verifiability** | Simple | Complex | Medium |
| **TEE compute** | Low | Higher (NP-hard) | Low |
| **WASM interface** | Submit bundle, get win/lose | Submit orders, get partial | Submit orders, get fills |

### TEE-Specific Analysis

**Auction (mev-boost style)**:
- WASM blob outputs: `(orders, bid)`
- TEE runs all blobs, picks highest bid winner
- Simple, fast, verified

**MWIS**:
- WASM blob outputs: `Vec<Order>`
- TEE must solve NP-hard MWIS
- Slower, harder to verify optimality

**Price-Priority**:
- WASM blob outputs: `Vec<Order>`
- TEE does simple sorting + matching
- Fast, easy to verify

### WASM Interface Design

For maximum flexibility, use **order-level interface**:

```rust
// What WASM blob sees
struct JitWasmInput {
    orderbook: AnonymizedOrderbook,
    clearing_prices: HashMap<MarketId, Price>,
    unfilled_demand: HashMap<MarketId, (Qty, Qty)>,  // (buy, sell)
}

// What WASM blob outputs
struct JitWasmOutput {
    orders: Vec<JitOrder>,
    // Optional: bundle constraints
    bundles: Vec<OrderBundle>,  // Groups that must fill together
}

struct OrderBundle {
    order_indices: Vec<usize>,  // Indices into orders
    min_fill_fraction: f64,     // 1.0 = all-or-nothing
}
```

This allows:
- Simple providers: Just output orders, accept partial fills
- Sophisticated providers: Bundle orders with constraints
- Selection mechanism can handle both

---

## Final Recommendation

### For Bootstrap (V1)

**Price-Priority Order Book** with optional bundling:

```rust
trait JitProvider {
    fn provide(&self, input: &JitInput) -> JitSubmission;
}

struct JitSubmission {
    orders: Vec<JitOrder>,
    // V1: ignore bundles, treat all as independent
    // V2: respect bundle constraints
    bundles: Vec<OrderBundle>,
}

// Selection: price-priority matching
fn select_jit(unfilled: &Unfilled, submissions: Vec<JitSubmission>) -> Vec<JitFill> {
    let all_orders = submissions.flat_map(|s| s.orders);
    price_priority_match(unfilled, all_orders)
}
```

### For Future (V2 with WASM)

**Hybrid**: Price-priority for independent orders, bundle-aware auction for bundled orders:

```rust
fn select_jit_v2(unfilled: &Unfilled, submissions: Vec<JitSubmission>) -> Vec<JitFill> {
    // Separate bundled vs independent orders
    let (bundles, independents) = partition_by_bundling(submissions);

    // Independent: price-priority
    let independent_fills = price_priority_match(unfilled, independents);
    let remaining = unfilled - independent_fills.volume();

    // Bundles: auction for remaining
    let bundle_fills = bundle_auction(remaining, bundles);

    concat(independent_fills, bundle_fills)
}
```

---

## Why Not MWIS?

MWIS is elegant but:

1. **Overkill for single-market**: Price-priority achieves same result more simply
2. **Doesn't handle partial fills naturally**: Need order splitting
3. **NP-hard**: Slower in TEE, harder to verify
4. **MMs don't think this way**: They think in limit orders, not conflict graphs

MWIS makes sense for **cross-market** JIT where orders genuinely conflict in non-obvious ways. For single-market, it's unnecessary complexity.

---

## Summary

| Mechanism | Best For | V1? | TEE-Ready? |
|-----------|----------|-----|------------|
| Simple Auction | Mutually exclusive bundles | No | Yes |
| MWIS | Cross-market conflicts | No | Harder |
| Price-Priority | Single-market orders | **Yes** | Yes |
| Hybrid | Mixed independent + bundles | V2 | Yes |

**V1 Recommendation**: Price-Priority Order Book
- Simple, well-understood, TEE-compatible
- Add bundle support in V2 if providers need it

---

## Future Consideration: Seat Auction

An alternative model closer to Arbitrum's Timeboost:

### Current Model (Per-Transaction Tax)
```
JIT providers pay tax on each displacement
Tax = f(displacement_volume, welfare_impact)
Anyone can participate, pay as you go
```

### Seat Auction Model
```
JIT "seats" are auctioned periodically (daily/weekly)
Winning bidders get exclusive JIT access for the period
Seat revenue goes to protocol or passive LPs
```

### Comparison

| Aspect | Per-Transaction Tax | Seat Auction |
|--------|--------------------|--------------|
| Entry barrier | Low (pay per use) | High (win auction) |
| Revenue predictability | Variable | Predictable |
| Competition | Per-batch | Per-period |
| Capital requirement | Low | High (seat cost) |
| Similarity | Unique | Timeboost-like |

### When Seat Auction Might Be Better

1. **Predictable protocol revenue**: Know income ahead of time
2. **Reduce per-batch complexity**: No tax calculation each batch
3. **Professional MM focus**: Only serious MMs bid for seats
4. **TEE simplification**: Fewer computations per batch

### Implementation Sketch

```rust
struct JitSeat {
    holder: ProviderId,
    valid_until: Timestamp,
    auction_price: Nanos,
}

struct SeatAuction {
    seats_available: u32,  // e.g., 3-5 seats
    auction_frequency: Duration,  // e.g., weekly
    min_bid: Nanos,
}

// JIT phase only accepts orders from seat holders
fn validate_jit_provider(provider: ProviderId, seats: &[JitSeat]) -> bool {
    seats.iter().any(|s| s.holder == provider && s.is_valid())
}
```

### Hybrid Option

Could combine both:
- Seats for "premium" JIT (lower/no per-tx tax)
- Open access with higher per-tx tax for non-seat-holders

This lets small MMs participate while giving volume discounts to committed providers.

**Decision**: Defer to V2/V3. Start with per-transaction tax for simplicity and inclusivity.
