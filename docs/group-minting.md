# Group Minting: Closing the MILP-Heuristic Gap

## The Problem

MILP beats all heuristic solvers by 4-5x on realistic scenarios. The gap is not a bug —
it's a structural capability that heuristic solvers lack entirely: **group-level minting**.

Per-market minting: pay $1, get 1 YES + 1 NO on one market.
Group minting: pay $1, get 1 YES on **every** market in a mutually exclusive group.

Group minting is N times cheaper per YES share because in a group of N mutually exclusive
outcomes, exactly one resolves YES — the $1 cost reflects the guaranteed $1 payout.

## What MILP Does

MILP has an explicit `group_mint_g` continuous variable per group:

```
Objective:  max Σ welfare_i(p,q) - Σ mint_m × $1 - Σ group_mint_g × $1

Position balance (market m in group g):
  YES: Σ buy_YES_demand = mint_m + group_mint_g
  NO:  Σ buy_NO_demand  = mint_m
```

The `group_mint_g` variable appears only in the YES balance. Each unit adds 1 YES share
to every market in the group. Cost: $1 per unit (not $1 per market).

This lets MILP fill buy-YES orders **that have no counterparty** by creating supply from
group minting. It can even set clearing prices to 0% on markets with little demand,
because the synthetic supply doesn't constrain the price.

### Concrete Example (from MILP tests)

3-candidate election. Only buy-YES orders: A@40c, B@35c, C@30c. No sellers at all.

- Heuristic: LocalSolver sees demand but no supply → no fills → $0 welfare
- MILP: `group_mint = 100` → fills all 3 → welfare = ($1.05 - $1.00) × 100 = **$5**

The $1.05 is the sum of buyer limits. The $1.00 is the group minting cost. The $0.05
per unit is pure welfare created from the arbitrage between individual willingness-to-pay
and the group structure.

## Why Heuristic Solvers Fail

### LocalSolver
Clears each market independently. Without sell orders, no fills. It has no concept
of cross-market supply — it doesn't know that cheap supply exists at the group level.

### NegriskSolver
Creates synthetic arb orders when `Σ p_YES < $1`. But:
1. Volume is bottlenecked: `max_shares = min(available_volume across all markets)`.
   `available_volume` comes from LocalSolver's fills. If any market has 0 fills → 0 arb.
2. Creates **demand** (the arbitrageur buying), not **supply**. The supply must
   already exist.

### DualMaster
Lambda-shades orders to push prices toward `Σ p = $1`. But shading only adjusts
willingness-to-pay of existing orders. It can't **create** supply. If there are no
sell-YES orders, no amount of lambda shading produces fills.

### Smoothed Solver
Tatonnement finds prices where excess demand ≈ 0. But with no sell-YES orders,
excess demand is just the full buy demand. The price converges to $1 (all buyers'
limit), not to a level that reflects group minting economics.

## The Core Insight

Group minting is a **virtual supply source**. It provides sell-YES liquidity on every
market in a group, backed by the exchange's ability to mint complete outcome sets.

The heuristic solvers decompose the problem into per-market subproblems. They can't see
that supply exists across markets simultaneously. The fix is to **make the virtual supply
visible to the per-market solver**.

## The Water-Filling Algorithm

### Formulation

For a group G with markets {m₁, ..., mₙ}, consider group-minting Q units.
Each unit provides 1 sell-YES share on every market, at cost $1.

For market m, let L_{m,1} ≥ L_{m,2} ≥ ... ≥ L_{m,Kₘ} be the unfilled buy-YES
limit prices sorted descending. At group_mint quantity Q:

- Fill Q orders on each market (the Q highest-limit buyers)
- Clearing price on market m: p_m = L_{m,Q} (the marginal buyer's limit)
- Total buyer value: Σ_m Σ_{i=1}^{Q} L_{m,i}
- Minting cost: Q × $1

**Net welfare:**
```
W(Q) = Σ_m Σ_{i=1}^{Q} L_{m,i} - Q × $1
```

Note: total surplus (buyer welfare + seller revenue) equals Σ L_{m,i}, independent
of clearing prices. The clearing price only redistributes between buyer and seller
(the "seller" being the minting operation). So we maximize total value minus cost.

### Optimality Condition

The marginal welfare of minting one more unit:

```
dW/dQ = Σ_m L_{m,Q+1} - $1
```

**Optimal Q\*: the largest Q such that Σ_m L_{m,Q} ≥ $1.**

In words: keep minting as long as the sum of the next marginal buyer's limit across
all markets exceeds $1 (the minting cost).

### Algorithm

```
function find_group_mint(group G, unfilled buy-YES orders):
    for each market m in G:
        demands[m] = sorted limits of unfilled buy-YES orders on m (descending)

    Q* = 0
    for Q = 1, 2, 3, ...:
        marginal_sum = Σ_m demands[m][Q]  (0 if Q > len(demands[m]))
        if marginal_sum < $1:
            break
        Q* = Q

    return Q*
```

Complexity: O(D log D) where D = total demand (dominated by sorting).

### Clearing Prices

Set p_m = L_{m,Q\*} for each market. This satisfies UCP:
- Filled buyers: limit ≥ L_{m,Q\*} = p_m ✓ (they're above the marginal)
- Unfilled buyers: limit < L_{m,Q\*} = p_m ✓ (they're below the marginal)

The clearing price may be 0% on markets with very low demand — this is correct,
matching MILP behavior. The minting cost is accounted for separately, not in the
per-market price.

## Integration Into the Pipeline

### Where It Runs

Group minting should run **after LocalSolver** (which handles markets with real
two-sided flow) and operate on **residual unfilled demand**.

Two integration points:

**Option A: Inside DualMaster iteration loop** (recommended)
- After LocalSolver clears, before MM knapsack
- Uses current iteration's prices to identify unfilled demand
- Group mint fills participate in per-iteration UCP
- Benefits from DualMaster's price convergence across iterations

**Option B: Separate pipeline phase**
- Between price discovery and MM allocation
- Simpler to implement
- But doesn't benefit from DualMaster's iterative refinement

### Interaction with Existing Phases

**LocalSolver**: runs first, handles markets with real supply/demand.
Group minting only considers orders LocalSolver didn't fill.

**MM Allocation**: runs after group minting. MM orders can also benefit from
group-minted supply (they're just orders with budget constraints).

**enforce_ucp**: runs after the full pipeline. May reprice group-minted fills
at the final clearing price. Key consideration: if LocalSolver set price=30c
and group minting set price=15c on the same market, enforce_ucp must reconcile
to a single price. The lower price (15c) would be used (more fills survive),
but this might violate some seller limits from LocalSolver.

**Solution**: when group minting runs, it should update the market's clearing
price. Subsequent LocalSolver iterations (in DualMaster) will adjust.

## Position Balance

Group minting creates YES shares without NO shares. This violates per-market
position balance (YES_fills ≠ NO_fills). Two approaches:

### Approach A: Arb Orders (like NegriskSolver)

Create synthetic arb orders that represent the minting operation:
- N sell-YES orders (one per market, qty = Q\*, price = p_m)
- These are the "counterparty" for the real buy-YES orders

For position balance, these synthetic sellers need YES shares. Where from?
From the group minting operation itself. We represent this as:

For each unit of group_mint:
- The exchange mints 1 YES on each market (position: +N YES)
- The synthetic arb sells 1 YES on each market (position: -N YES)
- Net position: 0

The minting cost ($1 per unit) is booked as negative welfare on the arb orders.
Specifically, the first arb order in the group carries limit = $1 and fill_price =
Σ p_m (total revenue). Welfare = $1 - Σ p_m = the negrisk gap. Remaining arb
orders have limit = fill_price (zero individual welfare). This is exactly the
NegriskSolver convention.

### Approach B: Verifier-Level Group Minting

Add `group_mint: HashMap<GroupId, u64>` to the BlockWitness. The verifier's
position balance check accounts for group minting:

```
per-market YES balance: YES_demand = YES_supply + mint_m + group_mint_g
per-market NO balance:  NO_demand  = NO_supply  + mint_m
```

More explicit and cleaner for ZK proofs. But requires verifier changes.

**Recommendation**: Start with Approach A (arb orders). It works with the existing
verifier. Migrate to Approach B when the verifier is mature enough.

## Edge Cases

### Market with zero demand
If any market in the group has zero buy-YES orders, `demands[m][Q] = 0` for all Q.
The sum drops by that market's contribution. Group minting can still proceed if the
other markets' limits sum to ≥ $1.

But the linking constraint requires Q fills on EVERY market. If market m has 0 demand,
Q\* = 0 (can't fill 0 orders).

MILP handles this via the position balance: `0 = mint_m + group_mint_g`, so
`mint_m = -group_mint_g` (negative mint = the exchange absorbs the minted share).
In the heuristic, we'd need to create a synthetic "dump" order that buys YES at
price 0 — essentially throwing away the minted YES share on the zero-demand market.

**The cost changes**: we mint Q units ($Q), sell YES on N-1 markets (revenue =
Σ_{m≠dead} p_m × Q), and waste 1 YES share per unit (no revenue on the dead market).
Profitable when Σ_{m≠dead} p_m > $1.

In practice, this rarely matters: scenario generators produce demand on all markets.
If it does matter, we can add a synthetic buy-YES at limit=0 on dead markets.

### Interaction with per-market minting
Per-market minting provides both YES and NO. It's needed when there's two-sided
demand but no natural supply (e.g., YES buyers and NO buyers but no sellers).
LocalSolver handles this implicitly.

Group minting provides only YES. The two are complementary:
- LocalSolver + per-market minting: handles two-sided demand
- Group minting: handles one-sided YES demand across the group

They don't conflict. The water-filling algorithm only considers demand that
LocalSolver left unfilled.

### Multiple groups
Each group is handled independently. An order can only be in one group.
No cross-group interaction needed.

### Group with 2 markets
Group minting on a 2-market group costs $1 for 2 YES shares ($0.50/share).
Per-market minting costs $1 for 1 YES + 1 NO ($1.00/share).
Group minting is 2x cheaper — still worthwhile.

### Negative welfare after minting cost
If Σ_m Σ_{i=1}^{Q\*} L_{m,i} < Q\* × $1, group minting has negative total welfare.
But by construction, Q\* is chosen so that the marginal unit has non-negative welfare
(Σ L_{m,Q\*} ≥ $1). So total welfare is guaranteed non-negative.

Proof: W(Q\*) = Σ_{q=1}^{Q\*} (Σ_m L_{m,q} - $1) ≥ 0, since each term ≥ 0.

## Comparison with Existing Approaches

| Aspect | NegriskSolver | Group Minting Phase | MILP |
|--------|--------------|---------------------|------|
| Creates supply? | No (demand only) | Yes (virtual supply) | Yes (group_mint_g) |
| Volume limit | min(existing fills) | min(demand across group) | Unlimited (continuous) |
| Cost accounting | Arb profit = gap | Explicit: Q × $1 | Objective term |
| Price setting | Takes from LocalSolver | Sets at marginal limit | Free variable |
| Position balance | Arb orders | Arb orders or verifier | Mint variables |
| Interaction | Iterative (adds demand) | Additive (fills residual) | Simultaneous |

## Remaining Gap After Group Minting

Even with group minting, the heuristic won't fully match MILP because:

1. **Joint optimization**: MILP optimizes prices, fills, minting, and MM budgets
   simultaneously. The heuristic pipeline is sequential (prices → fills → MM → minting).
   Each phase uses fixed inputs from the previous phase.

2. **Continuous relaxation**: MILP uses continuous group_mint (fractional shares).
   The heuristic uses integer shares (inherent to order-based matching).

3. **Bundle interactions**: MILP can jointly optimize bundle orders with group minting.
   The heuristic handles them in separate phases.

But group minting should capture the **dominant source** of the gap (4-5x → likely
1.5-2x residual), because the gap analysis showed most of MILP's advantage comes from
filling orders using group-minted supply at 0% prices.

## Implementation Sketch

```rust
/// Find optimal group minting quantity for a market group.
///
/// Returns (quantity, per-market clearing prices, arb orders).
fn find_group_mint(
    group: &MarketGroup,
    unfilled_buy_yes: &HashMap<MarketId, Vec<(u64, Nanos)>>,  // order_id, limit
    next_arb_id: &mut u64,
) -> Option<GroupMintResult> {
    let n = group.markets.len();
    if n < 2 { return None; }

    // Sort each market's demand descending by limit
    let mut demands: Vec<Vec<(u64, Nanos)>> = group.markets.iter()
        .map(|m| {
            let mut d = unfilled_buy_yes.get(m).cloned().unwrap_or_default();
            d.sort_by(|a, b| b.1.cmp(&a.1));
            d
        })
        .collect();

    // Water-filling: find Q* where Σ marginal limits ≥ $1
    let max_q = demands.iter().map(|d| d.len()).min().unwrap_or(0);
    let mut q_star = 0;

    for q in 0..max_q {
        let marginal_sum: u128 = demands.iter()
            .map(|d| d[q].1 as u128)
            .sum();
        if marginal_sum < NANOS_PER_DOLLAR as u128 {
            break;
        }
        q_star = q + 1;
    }

    if q_star == 0 { return None; }

    // Create fills and arb orders
    // ... (fills for real buy orders, arb sell orders for position balance)
}
```

## Next Steps

1. Implement `find_group_mint` as described above
2. Integrate into DualMaster iteration loop (after LocalSolver, before MM knapsack)
3. Create arb orders for position balance (NegriskSolver convention)
4. Test on small preset — expect dual welfare to jump from $1.6K toward $5-6K
5. Run `--solver all -v` gap analysis to measure remaining gap
6. Iterate: tune interaction with enforce_ucp, handle edge cases
