# Order Types: Detailed Catalog

## Background: TradFi Volume Context

| Order Type | TradFi Volume Share | Complexity | Our Priority |
|------------|---------------------|------------|--------------|
| Simple limit | ~60% | Low | Must have |
| Market order | ~25% | Low | Must have |
| Spread/pairs | ~10% | Medium | Must have |
| Multi-leg | ~4% | High | Should have |
| Exotic | ~1% | Very high | Nice to have |

---

## Type 1: Simple Limit Order

### Definition
"Buy/sell Q units at price P or better"

### TradFi Context
- Bread and butter of all exchanges
- ~60% of equity volume
- 100% of orderbook depth

### Prediction Market Examples
```
"Buy 100 shares of 'Trump wins 2024' at $0.45 or less"
"Sell 50 shares of 'BTC > $100k by Dec' at $0.60 or more"
```

### Linear Constraints
```
Variables:
  f = fill amount (0 ≤ f ≤ Q)
  p = execution price

Buy order:
  f ≤ Q                    (size limit)
  p ≤ P                    (price limit)
  f > 0 ⟹ p ≤ P           (only fill if price acceptable)

Sell order:
  f ≤ Q
  p ≥ P
  f > 0 ⟹ p ≥ P
```

In LP form:
```
For buy: fill × (price - limit) ≤ 0
For sell: fill × (limit - price) ≤ 0
```

### Value Calculation
```
Buyer surplus = f × (value - p) where value = user's true valuation
Seller surplus = f × (p - cost) where cost = user's true cost

For market making:
  Expected profit = spread × expected_volume × fill_probability
```

---

## Type 2: Market Order

### Definition
"Buy/sell Q units at any price"

### TradFi Context
- ~25% of volume
- Used for immediate execution
- Often from retail

### PM Examples
```
"Buy 100 shares of 'Harris wins' at any price"
"Sell my entire position in 'ETH > $5k'"
```

### Linear Constraints
```
Variables:
  f = fill amount

f ≤ Q           (size limit)
No price constraint
```

### Implementation Note
In batch auction, market orders are just limit orders with extreme limits:
- Buy market = buy limit @ $1.00
- Sell market = sell limit @ $0.00

---

## Type 3: Spread Order (2-Leg)

### Definition
"Buy A and sell B atomically, net cost ≤ budget"

### TradFi Context
- ~5% of options volume
- ~3% of futures volume
- Calendar spreads, pairs trading
- Very popular with sophisticated traders

### PM Examples

**Political correlation spread**:
```
"Buy 'Trump wins' + Sell 'GOP wins Senate'"
Thesis: If Trump wins, GOP almost certainly wins Senate
Net position: Long Trump-specific-win (Trump wins but GOP loses Senate)
```

**Hedged prediction**:
```
"Buy 'Lakers win championship' + Sell 'Lakers make playoffs'"
Thesis: Lakers making playoffs is more certain than championship
Net position: Championship premium over playoff
```

**Cross-event arbitrage**:
```
"Buy 'BTC > $100k Dec 2024' + Sell 'BTC > $80k Dec 2024'"
Thesis: 100k implies 80k, so this should have positive value
If mispriced, guaranteed profit
```

### Linear Constraints

```
Variables:
  f = fill amount (atomic, same for both legs)
  p_A = price in market A
  p_B = price in market B

Constraints:
  f ≤ Q_max                           (size limit)
  f × p_A - f × p_B ≤ budget          (net cost limit)
  f × p_A ≤ leg_A_limit × f           (per-leg price limit, optional)
  f × p_B ≥ leg_B_limit × f           (per-leg price limit, optional)
```

**The key constraint** (linearized):
```
f × (p_A - p_B) ≤ budget

If f and p are both variables, this is bilinear!
But in batch auction, p is determined by market clearing.
So we can treat p as "given" when evaluating fills.
```

### Solving Approach

For cross-market orders, the solver must:
1. Propose prices (p_A, p_B) that clear both markets
2. Check if spread order can fill at those prices
3. If yes, include in solution

This is why single-market is trivial but cross-market needs solvers.

### Value Calculation

**For correlation spread**:
```
True correlation: ρ
P(Trump) = 0.50
P(GOP Senate | Trump) = 0.90
P(GOP Senate | ¬Trump) = 0.40
P(GOP Senate) = 0.50 × 0.90 + 0.50 × 0.40 = 0.65

Fair spread price = P(Trump) - P(GOP Senate | Trump) × P(Trump)
                  = 0.50 - 0.90 × 0.50 = 0.05

If market spread is 0.10, there's 0.05 of edge.
```

---

## Type 4: Implication Order

### Definition
"If A, then B with this relationship"

You noted: "it's also implication order but one part inversed"

### The Insight

A spread order "Buy A, Sell B" is expressing:
```
I believe P(A) > P(B) by more than market implies
```

An implication order "A ⟹ B" expresses:
```
I believe P(B | A) is higher than market implies
```

**Mathematical relationship**:
```
P(A ∧ B) = P(A) × P(B|A)

Spread "Buy A, Sell B" profits if: P(A) - P(B) > current spread
Implication "A⟹B" profits if: P(B|A) > market's implied P(B|A)
```

### PM Examples

**Conditional prediction**:
```
"If Lakers make playoffs, then >60% chance they reach finals"
Order: Buy 'Lakers finals' @ 0.60 ONLY IF 'Lakers playoffs' fills
```

**Sequential events**:
```
"If Fed raises rates in March, buy 'recession by Dec' at 0.40"
Thesis: Rate hikes cause recessions with lag
```

### Linear Constraints

```
Variables:
  f_A = fill in market A (the condition)
  f_B = fill in market B (the implication)
  p_A, p_B = prices

Constraints:
  f_B ≤ f_A × ratio        (B only if A)
  f_B × p_B ≤ budget_B     (B cost limit)
  f_A × p_A ≤ budget_A     (A cost limit, might be 0)
```

**Conditional fill constraint**:
```
f_B ≤ M × I_A    where I_A = indicator "A filled"
M = big number

In LP: Can model with binary variables, or...
In batch: Just check after solving: if A didn't fill, don't fill B
```

---

## Type 5: Butterfly Order (3-Leg)

### Definition
"Buy low, sell 2× middle, buy high" - betting on certainty

### TradFi Context
- Classic options strategy
- ~1% of options volume
- Used to bet on low volatility

### PM Examples

**Election certainty bet**:
```
Markets: "Trump gets <40% vote", "Trump gets 40-50%", "Trump gets >50%"

Butterfly: Buy <40%, Sell 2× (40-50%), Buy >50%

Payoff:
  If <40%: Win big
  If 40-50%: Lose (sold 2 units)
  If >50%: Win big

Thesis: Election will be decisive, not close
```

**Price certainty bet**:
```
Markets: "BTC <$80k", "BTC $80k-$100k", "BTC >$100k"

Butterfly: Buy <80k, Sell 2× (80k-100k), Buy >100k

Thesis: BTC will move big, either crash or moon
```

### Linear Constraints

```
Variables:
  f = fill amount (scaled appropriately)
  p_low, p_mid, p_high = prices

Constraints:
  Buy f @ p_low
  Sell 2f @ p_mid
  Buy f @ p_high

  Net cost: f × p_low - 2f × p_mid + f × p_high ≤ budget
  Simplify: f × (p_low - 2×p_mid + p_high) ≤ budget
```

**Interesting property**: This is LINEAR in f once prices are known.

### Value Calculation (Black-Scholes Analogy)

In options, butterfly value depends on implied volatility:
```
Butterfly_value ≈ (σ_implied - σ_realized)² × vega
```

In prediction markets:
```
Let μ = expected outcome (e.g., expected vote share)
Let σ² = variance of outcome

Butterfly pays off when |outcome - μ| is large
Value ≈ P(|X - μ| > threshold) × payoff - cost

If market underestimates variance, butterfly is underpriced.
```

---

## Type 6: Iron Condor (4-Leg)

### Definition
"Sell middle range, buy protection on both tails"

### TradFi Context
- Popular options income strategy
- Sell volatility while limiting risk
- ~0.5% of options volume

### PM Example

```
Markets:
  A: "BTC < $70k"
  B: "BTC $70k-$90k"
  C: "BTC $90k-$110k"
  D: "BTC > $110k"

Iron Condor:
  Buy 1 A (tail protection)
  Sell 1 B (collect premium)
  Sell 1 C (collect premium)
  Buy 1 D (tail protection)

Payoff:
  If 70k-110k: Collect premium from B and C
  If <70k or >110k: Protected by A or D
```

### Linear Constraints

```
Variables:
  f = fill amount
  p_A, p_B, p_C, p_D = prices

Net cost: f × (p_A - p_B - p_C + p_D) ≤ budget
```

---

## Type 7: Ratio Spread

### Definition
"Buy X units of A, sell Y units of B" where X ≠ Y

### TradFi Context
- Used for leveraged directional bets
- Hedge non-linear exposures
- ~1% of options volume

### PM Examples

**Leveraged correlation**:
```
"Buy 2 'Trump wins', Sell 1 'GOP wins Senate'"

Thesis: Trump winning has 2× the impact on GOP Senate than market thinks
```

**Partial hedge**:
```
"Buy 100 'Lakers championship', Sell 50 'Lakers playoffs'"

Only 50% hedged - still want some Lakers exposure
```

### Linear Constraints

```
Variables:
  f_A, f_B = fill amounts
  p_A, p_B = prices

Constraints:
  f_A = ratio × f_B          (fixed ratio)
  f_A × p_A - f_B × p_B ≤ budget
  f_A ≤ max_A
  f_B ≤ max_B
```

---

## Type 8: Basket Order

### Definition
"Buy this portfolio of N markets"

### TradFi Context
- ETF creation/redemption
- Index arbitrage
- ~2% of equity volume

### PM Examples

**Election portfolio**:
```
"Buy 'Trump wins' + 'GOP Senate' + 'GOP House' as bundle"

Single order to get exposure to full GOP sweep
More capital efficient than 3 separate orders
```

**Sector bet**:
```
"Buy: 'ETH > $5k' + 'SOL > $300' + 'AVAX > $100'"

Betting on L1 altcoin rally as group
```

### Linear Constraints

```
Variables:
  f = fill amount (same for all, or weighted)
  p_1, ..., p_N = prices

Constraints:
  For each market i: fill_i = w_i × f (weighted fill)
  Total cost: Σ w_i × f × p_i ≤ budget
  Each fill ≤ size limit
```

### Value: Portfolio Theory

```
Expected return = Σ w_i × E[r_i]
Variance = Σ_i Σ_j w_i × w_j × Cov(r_i, r_j)

Optimal weights (Markowitz): minimize variance for given expected return
```

---

## Type 9: Contingent Order

### Definition
"Execute B only if A reaches price threshold"

### TradFi Context
- Stop-loss orders
- Take-profit orders
- OCO (one-cancels-other)

### PM Examples

**Stop-loss**:
```
"If 'Trump wins' drops below $0.40, sell my position"
```

**Take-profit**:
```
"If 'BTC > $100k' rises above $0.70, sell half"
```

**One-cancels-other**:
```
Order A: "Buy 'Harris wins' @ $0.45"
Order B: "Buy 'Trump wins' @ $0.48"
OCO: Only one can fill
```

### Linear Constraints

**For OCO**:
```
Variables:
  f_A, f_B = fills
  I_A, I_B = binary indicators

Constraints:
  I_A + I_B ≤ 1              (at most one fills)
  f_A ≤ M × I_A              (A fills only if I_A)
  f_B ≤ M × I_B              (B fills only if I_B)
```

**For threshold trigger**:
```
Trigger: Execute B if p_A ≤ threshold

In batch auction: After solving, check if p_A ≤ threshold
If yes, B is active; if no, B is cancelled
```

---

## Type 10: Pegged Order

### Definition
"Buy at (market price - offset)"

### TradFi Context
- Market making
- Following the market
- ~3% of equity volume

### PM Examples

```
"Buy 'Trump wins' at mid-market minus $0.02"
Always willing to be the best bid, 2 cents below current mid
```

### Linear Constraints

In CLOB: Price updates continuously
In batch auction: "Mid-market" is the clearing price

```
Variables:
  f = fill
  p = execution price (= clearing price)

Constraints:
  p ≤ clearing_price - offset    (buy peg)
  or
  p ≥ clearing_price + offset    (sell peg)
```

**This is tricky**: The constraint references the clearing price, which is endogenous.

**Solution**: Iterate or approximate
1. Solve without pegged orders
2. Get clearing price
3. Activate pegged orders at appropriate prices
4. Re-solve

---

## Summary: Implementation Priority

| Type | Complexity | Markets | Priority | Cross-market? |
|------|------------|---------|----------|---------------|
| Simple limit | O(1) | 1 | P0 | No |
| Market | O(1) | 1 | P0 | No |
| Spread (2-leg) | O(1) | 2 | P0 | Yes |
| Butterfly (3-leg) | O(1) | 3 | P1 | Yes |
| Iron Condor (4-leg) | O(1) | 4 | P1 | Yes |
| Ratio spread | O(1) | 2 | P1 | Yes |
| Basket | O(N) | N | P1 | Yes |
| Implication | O(1) | 2 | P2 | Yes |
| Contingent/OCO | Binary vars | 2+ | P2 | Yes |
| Pegged | Iterative | 1 | P2 | No |

**P0**: Must have at launch
**P1**: Important for sophisticated users
**P2**: Nice to have

---

## Appendix: Option Pricing Analogies

### Black-Scholes for Binary Options

A prediction market share is essentially a binary option:
- Payoff = $1 if event occurs, $0 otherwise
- Like a digital/binary call option

**Black-Scholes for binary**:
```
Price = e^(-rT) × N(d2)

where d2 = (ln(S/K) + (r - σ²/2)T) / (σ√T)
```

For prediction markets, think of:
- S = current probability estimate
- K = 0.5 (threshold)
- σ = uncertainty in probability estimate
- T = time to resolution

### Spread Pricing

For a spread (long A, short B):
```
Spread_value = P(A) - P(B)

If A and B are correlated:
Var(Spread) = Var(A) + Var(B) - 2×Cov(A,B)

Lower correlation → higher spread variance → higher option value on spread
```

### Butterfly Pricing

Butterfly value depends on probability distribution shape:
```
Butterfly_value = P(extreme outcome) × payoff_extreme - cost

In continuous analog:
Value ≈ ∫ payoff(x) × f(x) dx - premium

Where f(x) is the PDF of the outcome
```

Butterfly is essentially a bet on the tails of the distribution.
