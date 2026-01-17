# Solver Architecture

## External vs Internal Solvers

### Decision: External Solvers (Primary)

**Architecture**:
```
Orders → Sequencer → Batch seals → Engine publishes anonymized orderbook
                                          ↓
                          External solvers (TLS connection)
                                          ↓
                          Solutions submitted → Engine validates → Execute
```

### Why External Works

**Privacy preserved**:
- Orderbook revealed only AFTER batch seals
- Orders are anonymized (ephemeral IDs, no user identity)
- Solvers can't front-run (batch already sealed)
- Solvers can't add orders to this batch

**What solvers see**:
- Market IDs
- Order sides, sizes, prices
- Cross-market constraints
- NOT: user identities, balances, history

**TLS protects solver submissions**:
- Solver ↔ Engine connection encrypted
- Other solvers can't see each other's solutions
- No solution front-running

### What Internal Solvers Would Add

**Potential benefits**:
- Zero network latency
- Could see more (user IDs, balances)
- Guaranteed availability

**But we don't need these**:
- Latency: External can still be <100ms with good infra
- More info: Not useful for solving (user ID doesn't help match orders)
- Availability: Can have protocol fallback solver

### When Internal Makes Sense

**Community/Delegation Pools**:
- Users delegate funds to a strategy
- Strategy must be internal (holds user funds)
- Can't send funds to external solver

**Architecture for pools**:
```
Pool strategy (internal, in TEE):
  - Sees pool balances
  - Generates orders on behalf of pool
  - Orders go into batch like normal
  - External solvers see these orders (anonymized)
```

So internal strategies GENERATE orders, external solvers MATCH them.

### Hybrid Model

```
┌─────────────────────────────────────────────┐
│                    TEE                       │
│  ┌─────────────────────────────────────┐    │
│  │  Internal Strategy Execution        │    │
│  │  - Pool strategies                  │    │
│  │  - Generate orders for pools        │    │
│  └──────────────┬──────────────────────┘    │
│                 ↓                            │
│  ┌─────────────────────────────────────┐    │
│  │  Order Collection                   │    │
│  │  - User orders (encrypted)          │    │
│  │  - Pool orders (from strategies)    │    │
│  └──────────────┬──────────────────────┘    │
│                 ↓                            │
│  ┌─────────────────────────────────────┐    │
│  │  Batch Seals                        │    │
│  │  - Anonymize orders                 │    │
│  │  - Publish to external solvers      │    │
│  └──────────────┬──────────────────────┘    │
└─────────────────┼───────────────────────────┘
                  ↓
    ┌─────────────────────────────────────┐
    │  External Solvers                    │
    │  - Receive anonymized orderbook      │
    │  - Compute solutions                 │
    │  - Submit via TLS                    │
    └──────────────┬──────────────────────┘
                   ↓
┌─────────────────────────────────────────────┐
│                    TEE                       │
│  ┌─────────────────────────────────────┐    │
│  │  Solution Validation & Execution    │    │
│  │  - Validate solutions               │    │
│  │  - Select best                      │    │
│  │  - Execute batch                    │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

---

## Concrete Solver Strategies

### Solver 1: Protocol Baseline

**Purpose**: Fallback, always available, reasonable quality

**Strategy**: Exact single-market + greedy cross-market

```python
def solve_baseline(orderbook):
    # Step 1: Solve each market independently
    base_prices = {}
    base_fills = {}

    for market_id, orders in orderbook.by_market():
        if market.is_binary():
            price, fills = solve_binary_fba(orders)
        else:
            price, fills = solve_multi_outcome_lp(orders)
        base_prices[market_id] = price
        base_fills.update(fills)

    # Step 2: Greedily fill cross-market orders
    cross_orders = sorted(
        orderbook.cross_market_orders(),
        key=lambda o: o.potential_welfare(),
        reverse=True
    )

    for order in cross_orders:
        if can_fill_at_prices(order, base_prices):
            add_fills(base_fills, order, base_prices)
        else:
            # Try minimal price adjustment
            adjustment = find_minimal_adjustment(order, base_prices)
            if adjustment and adjustment.welfare_delta > 0:
                apply_adjustment(base_prices, adjustment)
                add_fills(base_fills, order, base_prices)

    return Solution(base_prices, base_fills)
```

**Complexity**: O(m × n log n) for single-market + O(k × m) for cross-market
**Quality**: Good baseline, not optimal for complex cross-market

---

### Solver 2: Full LP Solver

**Purpose**: Optimal solution for LP-representable orderbooks

**Strategy**: Formulate entire batch as single LP

```python
def solve_full_lp(orderbook):
    """
    Variables:
      p[m] = clearing price for market m
      f[i] = fill fraction for order i (0 to 1)

    Objective:
      maximize Σ_i surplus_i(f[i], p)

    Constraints:
      Market clearing: Σ buys = Σ sells for each market
      Price limits: f[i] > 0 implies price acceptable
      Cross-market: joint constraints on related orders
    """

    model = Model()

    # Variables
    prices = {m: model.var(0, 1) for m in orderbook.markets()}
    fills = {o.id: model.var(0, o.size) for o in orderbook.orders()}

    # Objective: maximize welfare
    welfare = sum(
        order.surplus(fills[o.id], prices[o.market])
        for o in orderbook.orders()
    )
    model.maximize(welfare)

    # Market clearing constraints
    for market in orderbook.markets():
        buys = sum(fills[o.id] for o in market.buy_orders())
        sells = sum(fills[o.id] for o in market.sell_orders())
        model.add(buys == sells)

    # Price limit constraints (indicator)
    for order in orderbook.orders():
        if order.is_buy():
            # If filled, price must be ≤ limit
            model.add(fills[order.id] <= order.size * (order.limit >= prices[order.market]))
        else:
            model.add(fills[order.id] <= order.size * (order.limit <= prices[order.market]))

    # Cross-market constraints
    for order in orderbook.cross_market_orders():
        add_cross_market_constraints(model, order, fills, prices)

    model.solve()

    return Solution(
        prices={m: prices[m].value for m in orderbook.markets()},
        fills={o.id: fills[o.id].value for o in orderbook.orders()}
    )
```

**Complexity**: LP solve, practically O(n²) to O(n³)
**Quality**: Optimal for LP-representable (no integer constraints)

---

### Solver 3: JIT Specialist

**Purpose**: Provide liquidity where base solution leaves gaps

**Strategy**: Analyze unfilled demand, propose profitable JIT

```python
def generate_jit_bids(orderbook, base_solution, mm_state):
    """Generate JIT bids for market maker."""
    jit_bids = []

    # Find unfilled orders
    for order in orderbook.orders():
        fill = base_solution.fill(order.id)
        unfilled = order.size - fill.amount

        if unfilled <= 0:
            continue

        # Can MM provide counterparty?
        if order.is_buy():
            # MM would sell
            mm_cost = mm_state.cost_to_sell(order.market, unfilled)
            potential_revenue = base_solution.price(order.market) * unfilled
            profit = potential_revenue - mm_cost
        else:
            # MM would buy
            mm_cost = base_solution.price(order.market) * unfilled
            potential_value = mm_state.value_of_buying(order.market, unfilled)
            profit = potential_value - mm_cost

        if profit > MIN_PROFIT_THRESHOLD:
            jit_order = Order(
                market=order.market,
                side=opposite(order.side),
                size=unfilled,
                price=base_solution.price(order.market)  # Match clearing
            )

            # Compute required fee
            welfare_delta = compute_welfare_delta(base_solution, jit_order)
            required_fee = FEE_STATE.compute_required_fee(welfare_delta)

            if profit > required_fee:
                jit_bids.append(JITBid(
                    orders=[jit_order],
                    fee_bid=required_fee * 1.1,  # Bid slightly above minimum
                ))

    return jit_bids
```

**Not a full solver**: Supplements other solvers with liquidity

---

### Solver 4: Arbitrage Hunter

**Purpose**: Find cross-market mispricings, improve price consistency

**Strategy**: Build price graph, find inconsistencies, propose corrections

```python
def generate_arb_patches(orderbook, base_solution):
    """Find and close arbitrage opportunities."""
    patches = []

    # Build implication graph
    # Edge A→B means "A implies B" with some probability
    graph = build_implication_graph(orderbook.markets())

    # Find price inconsistencies
    for (m1, m2) in graph.edges():
        p1 = base_solution.price(m1)
        p2 = base_solution.price(m2)

        implied_p2 = graph.implied_price(m1, m2, p1)
        mispricing = implied_p2 - p2

        if abs(mispricing) > MIN_ARB_THRESHOLD:
            # Construct arb orders
            if mispricing > 0:
                # p2 underpriced relative to p1
                # Buy m2, sell m1
                arb_orders = [
                    Order(m2, "buy", ARB_SIZE, p2 + mispricing/2),
                    Order(m1, "sell", ARB_SIZE, p1 - mispricing/2),
                ]
            else:
                # p2 overpriced
                arb_orders = [
                    Order(m2, "sell", ARB_SIZE, p2 + mispricing/2),
                    Order(m1, "buy", ARB_SIZE, p1 - mispricing/2),
                ]

            patches.append(Patch(
                affected_markets={m1, m2},
                orders=arb_orders,
                welfare_delta=abs(mispricing) * ARB_SIZE,
            ))

    return patches


def build_implication_graph(markets):
    """Build graph of market implications.

    Examples:
    - "Trump wins" → "GOP wins Senate" (high probability)
    - "BTC > 100k" → "BTC > 80k" (certainty)
    - "Lakers win championship" → "Lakers make playoffs" (certainty)
    """
    graph = Graph()

    for m1 in markets:
        for m2 in markets:
            if m1 == m2:
                continue

            # Check for logical implications
            implication = detect_implication(m1, m2)
            if implication:
                graph.add_edge(m1, m2, implication.probability)

    return graph
```

**Value**: Improves price consistency, captures arb for protocol/users

---

### Solver 5: Patch Combiner

**Purpose**: Combine patches from multiple solvers efficiently

**Strategy**: MWIS on conflict graph

```python
def combine_patches(patches, base_solution):
    """Select best non-conflicting patches."""

    # Build conflict graph
    conflicts = defaultdict(set)
    for i, p1 in enumerate(patches):
        for j, p2 in enumerate(patches):
            if i >= j:
                continue
            if p1.affected_markets & p2.affected_markets:
                conflicts[i].add(j)
                conflicts[j].add(i)

    # Score each patch
    scores = [p.welfare_delta for p in patches]

    # Solve MWIS (use randomized greedy for speed)
    selected = randomized_mwis(patches, conflicts, scores, iterations=1000)

    return [patches[i] for i in selected]


def randomized_mwis(items, conflicts, scores, iterations):
    """Randomized greedy for Maximum Weight Independent Set."""
    best_selected = []
    best_score = 0

    for _ in range(iterations):
        # Random permutation
        order = list(range(len(items)))
        random.shuffle(order)

        selected = []
        used = set()

        for i in order:
            if not (conflicts[i] & used):
                selected.append(i)
                used.add(i)
                used.update(conflicts[i])

        score = sum(scores[i] for i in selected)
        if score > best_score:
            best_score = score
            best_selected = selected

    return best_selected
```

---

## Solver Economics

### How Solvers Make Money

1. **Fee share**: Protocol shares batch fees with winning solver
2. **JIT profit**: Solver provides liquidity, captures spread
3. **Arb capture**: Solver closes arbs, keeps profit

### Fee Distribution

```
Batch fees collected: $100

Distribution:
- Protocol: 30% ($30)
- Winning solver: 50% ($50)
- JIT rebates: 20% ($20)
```

### Solver Competition

**What solvers compete on**:
- Solution quality (welfare)
- Speed (faster = more iterations possible)
- Specialization (unique patches others don't find)

**Not winner-take-all**:
- Multiple solvers can contribute patches
- JIT is separate from solving
- Arb hunting is separate specialty

---

## Implementation Priority

| Solver | Priority | Complexity | Notes |
|--------|----------|------------|-------|
| Baseline | P0 | Low | Must have fallback |
| Full LP | P0 | Medium | Core solving |
| JIT Specialist | P1 | Medium | Key for liquidity |
| Arb Hunter | P2 | Medium | Improves prices |
| Patch Combiner | P1 | Low | Needed for multi-solver |
