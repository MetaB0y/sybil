# Order Types

## Overview

Orders in Sybil V2 are represented as linear constraints over market outcomes. This enables a rich set of order types beyond simple limit orders.

## Implementation

Orders are defined in `matching-engine/src/order.rs`:

```rust
pub struct Order {
    pub id: u64,
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],
    pub num_markets: u8,
    pub payoffs: [i8; MAX_STATES],  // Payoff per joint state
    pub limit_price: Nanos,          // Max cost per unit
    pub min_fill: Qty,
    pub max_fill: Qty,
    pub condition: Option<PriceCondition>,
}
```

The `payoffs` array defines the value the order holder receives in each possible state. For multi-market orders, states are the Cartesian product of individual market outcomes.

---

## Order Types

### Type 1: Simple Limit Order

"Buy/sell Q units at price P or better"

**Use case**: Standard trading, ~85% of volume

**Implementation**:
```rust
// Buy YES on binary market
simple_yes_buy(markets, id, market, limit_price, qty)
// payoffs = [1, 0]  -- win if outcome 0

// Buy NO on binary market
simple_no_buy(markets, id, market, limit_price, qty)
// payoffs = [0, 1]  -- win if outcome 1
```

**Constraint**:
```
fill × (clearing_price - limit_price) ≤ 0  (for buy)
fill × (limit_price - clearing_price) ≤ 0  (for sell)
```

---

### Type 2: Multi-Outcome Position

"Buy outcome K in a multi-outcome market"

**Use case**: Markets with 3+ outcomes (e.g., "Who wins: Trump/Harris/Other")

**Implementation**:
```rust
outcome_buy(markets, id, market, outcome_idx, limit_price, qty)
// payoffs = [0, ..., 1, ..., 0]  -- 1 at outcome_idx
```

---

### Type 3: Spread Order (2-Leg)

"Buy A and sell B atomically, net cost ≤ budget"

**Use case**: Correlation trades, hedging

**Examples**:
- "Long Trump, Short GOP Senate" - bet on Trump-specific effect
- "Long Lakers Championship, Short Lakers Playoffs" - championship premium

**Implementation**:
```rust
spread(markets, id, market_a, market_b, limit_price, qty)
// For two binary markets A and B (4 joint states):
// payoffs = [0, -1, +1, 0]
//   A=Yes, B=Yes: 0 (both won, net zero)
//   A=No,  B=Yes: -1 (short B lost)
//   A=Yes, B=No:  +1 (long A won)
//   A=No,  B=No:  0 (both lost, net zero)
```

**Constraint**:
```
fill × (price_A - price_B - limit) ≤ 0
```

---

### Type 4: Bundle Order

"Buy YES on multiple markets (all must win)"

**Use case**: Parlay bets, correlated event bundles

**Example**: "Trump wins AND GOP Senate AND GOP House"

**Implementation**:
```rust
bundle_yes(markets, id, &[market_a, market_b], limit_price, qty)
// payoffs = [1, 0, 0, 0]  -- only win if all outcomes are 0 (Yes)
```

This is an all-or-none order: `min_fill == max_fill`.

---

### Type 5: Butterfly Order (3-Leg)

"Bet on extremes vs middle"

**Use case**: Volatility/certainty bets

**Example**: Election will be decisive (landslide either way), not close

**Implementation**:
```rust
butterfly(markets, id, market, limit_price, qty)
// For 3-outcome market [Low, Mid, High]:
// payoffs = [+1, -2, +1]
// Win on extremes, lose on middle
```

---

### Type 6: Ratio Spread

"Buy N units of A, sell M units of B"

**Use case**: Leveraged correlation bets

**Implementation**:
```rust
ratio_spread(markets, id, market_a, ratio_a, market_b, ratio_b, limit, qty)
// payoffs encode the ratio relationship
```

---

### Type 7: Conditional Order

"Activate only when another market crosses price threshold"

**Use case**: Stop-loss, take-profit, sequenced bets

**Implementation**:
```rust
conditional_buy(markets, id, market, limit, qty, cond_market, threshold, direction)

pub enum ConditionDir {
    Above,  // Activate when cond_market price > threshold
    Below,  // Activate when cond_market price < threshold
}
```

The condition is evaluated when the batch is sealed. Orders failing their condition are excluded from solving.

---

## State Space

For multi-market orders, the `payoffs` array indexes over the joint state space:

```rust
// 2 binary markets: 2×2 = 4 states
// State 0: (A=0, B=0) - both Yes
// State 1: (A=1, B=0) - A No, B Yes
// State 2: (A=0, B=1) - A Yes, B No
// State 3: (A=1, B=1) - both No

// 3 markets with 2, 3, 2 outcomes: 2×3×2 = 12 states
// State index = a + size_a × (b + size_b × c)
```

The `StateSpace` struct handles index calculation.

---

## Order Builder

The `OrderBuilder` provides a fluent API:

```rust
let order = OrderBuilder::new(markets, id)
    .spanning(&[market_a, market_b])
    .limit(price_to_nanos(0.60))
    .quantity(0, 100)
    .payoff_when(&[0, 0], 1)  // Win when both Yes
    .build();
```

---

## Priority & Complexity

| Type | Markets | Complexity | Status |
|------|---------|------------|--------|
| Simple limit | 1 | Low | Implemented |
| Multi-outcome | 1 | Low | Implemented |
| Spread (2-leg) | 2 | Medium | Implemented |
| Bundle | N | Medium | Implemented |
| Butterfly (3-leg) | 1 (3+ outcomes) | Medium | Implemented |
| Ratio spread | 2 | Medium | Implemented |
| Conditional | 1+cond | Medium | Implemented |

---

## Welfare Calculation

Order welfare (surplus) is:
```rust
impl Fill {
    pub fn welfare(&self, order: &Order) -> i64 {
        let expected_payoff = self.expected_payoff(order);
        let cost = self.total_cost();
        expected_payoff - cost
    }
}
```

For filled orders:
- **Buyer welfare** = (payoff value - price paid) × quantity
- **Seller welfare** = (price received - payoff cost) × quantity

Total batch welfare = sum of all order welfare.
