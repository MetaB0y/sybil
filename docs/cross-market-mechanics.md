# Cross-Market Order Matching: A Mechanistic Analysis

## Executive Summary

Cross-market orders let traders express views on **joint outcomes** across multiple prediction markets. This document analyzes:
1. What cross-market orders actually represent
2. How matching would work mechanistically
3. Whether this adds meaningful value vs. complexity

**Key Finding**: Cross-market orders create genuinely new securities that cannot be synthesized from single-market positions. However, the practical value depends heavily on market correlation structure and trader sophistication.

---

## Part 1: The Economic Foundation

### Single-Market Orders

In a single market with outcomes {A, B}:
- "Buy A at $0.40" means: pay $0.40, receive $1 if A happens
- Payoff vector: `[+1, 0]` for outcomes [A, B]

### Cross-Market Orders

With two binary markets (Rain: {R, ¬R}, Concert: {C, ¬C}), there are 4 joint outcomes:
- RC: Rain AND Concert cancelled
- R¬C: Rain AND Concert happens
- ¬RC: No rain AND Concert cancelled
- ¬R¬C: No rain AND Concert happens

A cross-market order "Buy (Rain AND Cancel)" means:
- Pay price P
- Receive $1 if BOTH rain AND cancel (outcome RC)
- Receive $0 otherwise
- Payoff vector: `[+1, 0, 0, 0]` over joint outcomes [RC, R¬C, ¬RC, ¬R¬C]

### Why These Are Different Securities

**Buying Rain-YES ($0.30) + Cancel-YES ($0.40) separately:**
| Outcome | Rain Payoff | Cancel Payoff | Total | Cost | Net |
|---------|-------------|---------------|-------|------|-----|
| RC      | +$1         | +$1           | $2    | $0.70| +$1.30 |
| R¬C     | +$1         | $0            | $1    | $0.70| +$0.30 |
| ¬RC     | $0          | +$1           | $1    | $0.70| +$0.30 |
| ¬R¬C    | $0          | $0            | $0    | $0.70| -$0.70 |

**Buying "Rain AND Cancel" cross-market at $0.25:**
| Outcome | Cross Payoff | Cost | Net |
|---------|--------------|------|-----|
| RC      | +$1          | $0.25| +$0.75 |
| R¬C     | $0           | $0.25| -$0.25 |
| ¬RC     | $0           | $0.25| -$0.25 |
| ¬R¬C    | $0           | $0.25| -$0.25 |

**These are fundamentally different payoff profiles.** You cannot replicate the cross-market order by combining single-market orders.

---

## Part 2: The Joint Outcome Market Model

### Full Representation

For N binary markets, there are 2^N joint outcomes. Each can be its own "market":

```
Market structure for 2 binary questions:

Joint Market 1: "RC" (Rain AND Cancel)       → P(RC)
Joint Market 2: "R¬C" (Rain AND No-Cancel)   → P(R¬C)
Joint Market 3: "¬RC" (No-Rain AND Cancel)   → P(¬RC)
Joint Market 4: "¬R¬C" (No-Rain AND No-Cancel)→ P(¬R¬C)

Constraint: P(RC) + P(R¬C) + P(¬RC) + P(¬R¬C) = 1
```

### Marginal Consistency Constraints

If we also have single-outcome markets:
- P(Rain) = P(RC) + P(R¬C)
- P(Cancel) = P(RC) + P(¬RC)

These create **arbitrage relationships** between single and joint markets.

---

## Part 3: Matching Scenarios

### Scenario 1: Direct Cross-Market Match

**Setup:**
- Alice: "Buy (Rain AND Cancel) at $0.30"
- Bob: "Sell (Rain AND Cancel) at $0.25"

**Matching:**
This is trivial - same market, opposite sides. Clear at any price in [$0.25, $0.30].

```
Alice pays $0.27 → Bob
If RC: Bob pays $1 → Alice
If not RC: Nothing
```

**Verdict**: No special solver needed. Standard single-market matching.

---

### Scenario 2: Synthetic Matching via Complete Set

**Setup:**
- Alice: "Buy (Rain AND Cancel) at $0.30"
- No direct counterparty

**Can we synthesize from other markets?**

To create "pays $1 if RC" synthetically:
- Need to SHORT the other 3 outcomes
- Buy ¬(RC) = Buy (R¬C OR ¬RC OR ¬R¬C)

But wait - "Buy (R¬C OR ¬RC OR ¬R¬C)" is also a cross-market order!

**Key insight**: You CANNOT synthesize joint outcomes from marginal (single-market) orders alone.

**Verdict**: Cross-market liquidity requires cross-market counterparties.

---

### Scenario 3: Arbitrage Between Marginal and Joint Markets

**Setup:**
- Rain market: P(Rain) = $0.40
- Cancel market: P(Cancel) = $0.50
- Joint market: P(RC) = $0.30

**Check consistency:**
If Rain and Cancel were independent:
- P(RC) should = P(R) × P(C) = 0.40 × 0.50 = $0.20

But P(RC) = $0.30 > $0.20, implying positive correlation.

**Is there arbitrage?**

Only if marginal constraints are violated:
- P(R) = P(RC) + P(R¬C) → P(R¬C) = 0.40 - 0.30 = $0.10
- P(C) = P(RC) + P(¬RC) → P(¬RC) = 0.50 - 0.30 = $0.20
- P(¬R¬C) = 1 - 0.30 - 0.10 - 0.20 = $0.40

Check: P(¬R) = P(¬RC) + P(¬R¬C) = 0.20 + 0.40 = 0.60 ✓ (equals 1 - P(R))

**No arbitrage** - prices are consistent, they just imply correlation.

**When IS there arbitrage?**

If marginal markets say P(R) = $0.40, but joint markets sum to:
- P(RC) + P(R¬C) = $0.50 ≠ $0.40

Then arbitrage exists:
- Sell Rain-YES in marginal market at $0.40
- Buy RC + R¬C in joint market at $0.50... wait, that loses money
- Actually: Buy Rain-YES at $0.40, Sell (RC + R¬C) at $0.50 → profit $0.10

**Arbitrage trade:**
```
+1 Rain-YES (marginal)     cost: $0.40
-1 RC joint                receive: (P(RC) portion)
-1 R¬C joint               receive: (P(R¬C) portion)

If P(RC) + P(R¬C) = $0.50 and P(Rain) = $0.40:
Net: -$0.40 + $0.50 = +$0.10 risk-free profit
```

**Verdict**: A cross-market solver detects when marginal ≠ sum(joint) and executes the arb.

---

### Scenario 4: Correlation Trading

**Trader's view**: "Rain and Cancel are more correlated than market implies"

**Market state:**
- P(RC) = $0.12 (market implies weak correlation)
- P(R¬C) = $0.28
- P(¬RC) = $0.38
- P(¬R¬C) = $0.22
- Implied: P(R) = 0.40, P(C) = 0.50

**Independence would imply**: P(RC) = 0.20

Market says P(RC) = 0.12 < 0.20 → market thinks NEGATIVE correlation!

**Trader's trade** (if they believe positive correlation):
- Buy RC at $0.12
- Sell ¬R¬C at $0.22 (if no rain, likely concert happens)

This is a **spread trade** expressing a view on correlation, not on marginals.

**Matching requirement:**
- Need counterparty willing to sell RC
- Need counterparty willing to buy ¬R¬C
- These could be the SAME counterparty (opposite correlation view)

**Verdict**: Correlation trading is a legitimate use case but requires sophisticated traders and liquid joint markets.

---

### Scenario 5: Conditional Orders

**Order**: "Buy Cancel-YES, but only if Rain-YES also happens"

This is equivalent to: "Buy RC"

**Alternative order**: "If it rains, I want exposure to cancellation"
- Payoff: +$1 if RC, +$0 if R¬C, +$0 if ¬R anything
- This is just "Buy RC" again

**What about**: "Buy Cancel-YES at $0.40 IF Rain-YES settles first"
- This is a contingent order, not a cross-market order
- Only activates if Rain market resolves to YES
- Then becomes a normal Cancel-YES order

**Verdict**: Conditional orders are either:
1. Reducible to joint outcome orders (same market, different packaging)
2. Temporal contingencies (wait for resolution, then place order)

---

## Part 4: Cross-Market Solver Architecture

### What Would a Cross-Market Solver Actually Do?

**Function 1: Arbitrage Detection**
```
Input: Prices from marginal markets, prices from joint markets
Output: Arbitrage opportunities (if any)

Algorithm:
1. For each marginal market, compute sum of relevant joint prices
2. If P(marginal) ≠ Σ P(joint components): arbitrage exists
3. Construct offsetting position to capture spread
```

**Function 2: Synthetic Order Construction**
```
Input: Cross-market order that lacks direct counterparty
Output: Equivalent position from available liquidity (if possible)

Algorithm:
1. Express desired payoff as vector over joint outcomes
2. Check if payoff can be replicated from available orders
3. If yes, match against replicating portfolio
4. If no, order remains unfilled (needs direct counterparty)
```

**Function 3: Multi-Market Clearing**
```
Input: Orders across all markets (marginal + joint)
Output: Fills that maximize welfare subject to consistency

Algorithm:
1. Formulate LP with all orders
2. Add marginal consistency constraints
3. Solve for prices and fills simultaneously
4. Ensure no arbitrage in solution
```

### The LP Formulation

**Variables:**
- p_i: price of each joint outcome i
- q_j: fill quantity for each order j

**Constraints:**
- Σ p_i = 1 (probabilities sum to 1)
- For each marginal: p_marginal = Σ p_joint (consistency)
- For each order: q_j ≤ max_qty_j
- For each order: q_j = 0 OR satisfies limit price

**Objective:**
- Maximize Σ (limit_price - clearing_price) × q for buyers
- Plus Σ (clearing_price - limit_price) × q for sellers

---

## Part 5: Is Cross-Market Worth It?

### Arguments FOR:

1. **Expressiveness**: Traders can bet on correlations, not just marginals
2. **Efficiency**: Arbitrage keeps related markets consistent
3. **Hedging**: Can construct precise payoffs impossible with single markets
4. **Information**: Joint prices reveal correlation beliefs

### Arguments AGAINST:

1. **Complexity**: 2^N markets for N binary questions (exponential)
2. **Liquidity fragmentation**: Each joint market needs its own liquidity
3. **Thin markets**: Most joint outcomes have few interested traders
4. **Fee arbitrage**: $1 per market creation makes small arbs unprofitable

### When Cross-Market Adds Value:

| Scenario | Value | Reason |
|----------|-------|--------|
| Highly correlated markets | High | Correlation views are economically meaningful |
| Independent markets | Low | Joint = product of marginals, no new info |
| Sophisticated traders | High | Can express complex views |
| Retail traders | Low | Single-market bets suffice |
| Deep liquidity | High | Tight spreads enable arb |
| Thin liquidity | Low | Can't execute arb trades |

### The "Trade Correlations" Question

**Is trading correlations a good idea?**

Economically: Yes, correlations contain real information.
- "Will the Fed raise rates?" and "Will stocks fall?" are correlated
- A trader might believe they're MORE correlated than market implies
- That's a legitimate, valuable signal

Practically: Depends on market depth.
- If you can't find counterparties, the signal can't be expressed
- If spreads are wide, the value is eaten by costs

---

## Part 6: Implementation Recommendations

### Minimal Viable Cross-Market

Start with **arbitrage enforcement only**:
1. Compute consistency constraints between related markets
2. If prices violate constraints, execute rebalancing trades
3. Don't create new joint markets - just enforce consistency on existing ones

### Full Cross-Market

If traders demand it:
1. Allow explicit joint outcome orders
2. Implement the LP solver with consistency constraints
3. Consider maker rebates to incentivize joint-market liquidity

### Skip Cross-Market If:

1. Markets are mostly independent
2. Trader base is unsophisticated
3. Single-market liquidity is already thin

---

## Appendix: Worked Example

**Markets:**
- M1: "Trump wins election" (T/¬T)
- M2: "Stock market up >10% in 2025" (S/¬S)

**Single-market prices:**
- P(T) = $0.55
- P(S) = $0.30

**Joint market prices:**
- P(TS) = $0.22 (Trump wins AND stocks up)
- P(T¬S) = $0.33 (Trump wins AND stocks down)
- P(¬TS) = $0.08 (Trump loses AND stocks up)
- P(¬T¬S) = $0.37 (Trump loses AND stocks down)

**Check consistency:**
- P(T) = P(TS) + P(T¬S) = 0.22 + 0.33 = 0.55 ✓
- P(S) = P(TS) + P(¬TS) = 0.22 + 0.08 = 0.30 ✓

**Check correlation:**
- If independent: P(TS) = 0.55 × 0.30 = 0.165
- Actual: P(TS) = 0.22 > 0.165
- Market implies: Trump win is correlated with stocks up

**Correlation trade** (disagree with market):

"I think Trump is UNCORRELATED with stocks"

Trade:
- Sell TS at $0.22 (it's overpriced vs independence)
- Buy T at $0.55, Buy S at $0.30
- Construct hedge...

Actually this is complex. Cleaner trade:
- Sell TS at $0.22
- Buy T¬S at $0.33, Buy ¬TS at $0.08, Buy ¬T¬S at $0.37

Net position:
- If TS: owe $1 (sold), paid $0.22 + 0.33 + 0.08 + 0.37 = $1.00 for others → net $0
- Wait, that's just buying a complete set for $1.00

The trade is:
- Sell TS at $0.22
- If TS happens: lose $0.78 (pay $1, received $0.22)
- If not TS: profit $0.22

Expected value if independent (P(TS) = 0.165):
- E[profit] = 0.835 × $0.22 - 0.165 × $0.78 = $0.184 - $0.129 = +$0.055

**This is the correlation trade payoff**: profitable if market overestimates correlation.

---

## Conclusion

Cross-market matching is economically meaningful but practically challenging:

1. **It creates genuinely new securities** - not reducible to single-market combinations
2. **Arbitrage enforcement is straightforward** - just add consistency constraints
3. **Full cross-market liquidity is hard** - exponential market fragmentation
4. **Value depends on correlation structure** - high for correlated markets, low for independent

**Recommendation**: Start with consistency constraints in the solver, evaluate demand for explicit joint markets before building them.
