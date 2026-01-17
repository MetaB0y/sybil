# Simulation Framework: Detailed Plan

## Goals

1. **Validate architecture**: Does patch-based solving work?
2. **Measure quality**: How far from optimal? Path dependence impact?
3. **Compare to CLOB**: What do we gain/lose vs continuous matching?
4. **Test solver strategies**: Which approaches work best?
5. **Stress test**: What breaks at scale?

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    Simulation Framework                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────┐      ┌──────────────────┐                 │
│  │  Agent Factory   │      │  Market Factory  │                 │
│  │                  │      │                  │                 │
│  │  - LLM Agents    │      │  - Binary        │                 │
│  │  - MM Bots       │      │  - Multi-outcome │                 │
│  │  - Arb Bots      │      │  - Correlated    │                 │
│  │  - Noise Traders │      │  - Time-series   │                 │
│  └────────┬─────────┘      └────────┬─────────┘                 │
│           │                         │                            │
│           ▼                         ▼                            │
│  ┌─────────────────────────────────────────────┐                │
│  │              World State                     │                │
│  │  - Markets with "true" probabilities         │                │
│  │  - News/event stream                         │                │
│  │  - Agent portfolios                          │                │
│  └─────────────────────┬───────────────────────┘                │
│                        │                                         │
│                        ▼                                         │
│  ┌─────────────────────────────────────────────┐                │
│  │           Order Generation                   │                │
│  │  Each agent submits orders based on:         │                │
│  │  - Their beliefs (LLM or model)              │                │
│  │  - Current prices                            │                │
│  │  - Their portfolio                           │                │
│  └─────────────────────┬───────────────────────┘                │
│                        │                                         │
│                        ▼                                         │
│  ┌─────────────────────────────────────────────┐                │
│  │         Matching Engine (pluggable)          │                │
│  │  - BatchAuctionEngine                        │                │
│  │  - CLOBEngine                                │                │
│  │  - PeriodicCLOBEngine                        │                │
│  └─────────────────────┬───────────────────────┘                │
│                        │                                         │
│                        ▼                                         │
│  ┌─────────────────────────────────────────────┐                │
│  │              Metrics Collector               │                │
│  │  - Fill rate, welfare, spreads               │                │
│  │  - Price accuracy vs true probability        │                │
│  │  - Solver performance                        │                │
│  └─────────────────────────────────────────────┘                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Component 1: World State

### Purpose
Ground truth that agents trade against.

### Implementation

```python
@dataclass
class Market:
    id: str
    outcomes: List[str]          # ["Yes", "No"] for binary
    true_probabilities: List[float]  # Hidden from agents
    resolution_time: datetime
    category: str                # "politics", "sports", "crypto"
    correlations: Dict[str, float]  # Correlation with other markets

@dataclass
class WorldState:
    markets: Dict[str, Market]
    current_time: datetime
    news_stream: List[NewsEvent]

    def get_true_price(self, market_id: str, outcome: int) -> float:
        """Ground truth price (hidden from agents)."""
        return self.markets[market_id].true_probabilities[outcome]

    def generate_news(self) -> Optional[NewsEvent]:
        """Generate random news that affects true probabilities."""
        # News can shift true probabilities
        # Agents observe news and update beliefs (with noise)
        pass

@dataclass
class NewsEvent:
    timestamp: datetime
    affected_markets: List[str]
    probability_shifts: Dict[str, List[float]]  # How true probs change
    headline: str  # For LLM agents
```

### Market Correlation Structure

```python
def create_correlated_markets() -> Dict[str, Market]:
    """Create markets with realistic correlation structure."""

    # Election cluster
    trump_wins = Market("trump_wins", ["Yes", "No"], [0.48, 0.52], ...)
    gop_senate = Market("gop_senate", ["Yes", "No"], [0.55, 0.45], ...)
    gop_house = Market("gop_house", ["Yes", "No"], [0.52, 0.48], ...)

    # Correlations
    trump_wins.correlations = {"gop_senate": 0.7, "gop_house": 0.6}
    gop_senate.correlations = {"trump_wins": 0.7, "gop_house": 0.8}

    # Crypto cluster
    btc_100k = Market("btc_100k", ["Yes", "No"], [0.35, 0.65], ...)
    eth_5k = Market("eth_5k", ["Yes", "No"], [0.40, 0.60], ...)

    btc_100k.correlations = {"eth_5k": 0.85}

    # Uncorrelated
    lakers_championship = Market("lakers_championship", ...)
    # No correlations to political/crypto markets

    return {...}
```

### Effort Estimate
- Core data structures: 200 LOC
- Correlation logic: 150 LOC
- News generation: 200 LOC
- **Total: ~550 LOC, 1-2 days**

---

## Component 2: Agent Factory

### Agent Types

#### Type A: LLM Trader
Uses language model to make trading decisions.

```python
class LLMTrader(Agent):
    def __init__(self, persona: str, llm_client, balance: float):
        self.persona = persona
        self.llm = llm_client
        self.balance = balance
        self.portfolio = {}
        self.memory = []  # Past trades, P&L

    async def generate_orders(
        self,
        market_state: Dict[str, float],  # Current prices
        news: List[str],
        portfolio: Dict[str, int]
    ) -> List[Order]:

        prompt = f"""You are a {self.persona} prediction market trader.

Current market prices:
{format_prices(market_state)}

Recent news:
{format_news(news)}

Your current portfolio:
{format_portfolio(portfolio)}

Your balance: ${self.balance:.2f}

Based on your trading persona and the current situation, generate orders.
Consider:
- Your risk tolerance
- Correlation between markets
- News impact on probabilities
- Your existing positions

Output JSON array of orders:
[{{"market": "...", "side": "buy/sell", "size": N, "price": P}}, ...]
"""

        response = await self.llm.complete(prompt)
        return parse_orders(response)
```

**Persona examples**:
```python
PERSONAS = {
    "retail_gambler": """
        You're a casual bettor who follows politics and sports.
        You bet based on gut feeling and news headlines.
        You're risk-tolerant and like to make bold bets.
        You don't think much about correlations or hedging.
    """,

    "quant_trader": """
        You're a quantitative trader with statistics background.
        You look for mispricings and correlation opportunities.
        You prefer spread trades and hedged positions.
        You're risk-averse and size positions carefully.
    """,

    "news_trader": """
        You react quickly to news events.
        You believe news moves markets and early reaction is profitable.
        You prefer liquid markets where you can get in and out.
        You don't hold positions for long.
    """,

    "contrarian": """
        You bet against the crowd.
        When everyone is bullish, you look for shorts.
        You believe markets overreact to news.
        You're patient and willing to hold losing positions.
    """
}
```

#### Type B: Market Maker Bot
Algorithmic market making.

```python
class MarketMakerBot(Agent):
    def __init__(self,
                 target_spread: float,
                 inventory_limit: float,
                 risk_aversion: float):
        self.target_spread = target_spread
        self.inventory_limit = inventory_limit
        self.risk_aversion = risk_aversion

    def generate_orders(self, market_state, portfolio) -> List[Order]:
        orders = []

        for market_id, mid_price in market_state.items():
            current_inventory = portfolio.get(market_id, 0)

            # Skew quotes based on inventory
            skew = self.risk_aversion * current_inventory / self.inventory_limit

            bid_price = mid_price - self.target_spread/2 - skew
            ask_price = mid_price + self.target_spread/2 - skew

            # Size based on inventory room
            bid_size = max(0, self.inventory_limit - current_inventory)
            ask_size = max(0, self.inventory_limit + current_inventory)

            if bid_size > 0:
                orders.append(Order(market_id, "buy", bid_size, bid_price))
            if ask_size > 0:
                orders.append(Order(market_id, "sell", ask_size, ask_price))

        return orders
```

#### Type C: Arbitrage Bot
Looks for cross-market mispricings.

```python
class ArbitrageBot(Agent):
    def __init__(self, min_edge: float, correlations: Dict):
        self.min_edge = min_edge
        self.correlations = correlations

    def generate_orders(self, market_state, portfolio) -> List[Order]:
        orders = []

        # Check for correlation-based arbs
        for (m1, m2), corr in self.correlations.items():
            p1 = market_state[m1]
            p2 = market_state[m2]

            # Expected relationship
            expected_p2 = self.implied_price(p1, corr)

            edge = expected_p2 - p2
            if abs(edge) > self.min_edge:
                # Spread trade
                if edge > 0:  # p2 is underpriced
                    orders.append(SpreadOrder(
                        legs=[(m1, "sell"), (m2, "buy")],
                        size=self.calculate_size(edge),
                        max_cost=edge * 0.5  # Keep half the edge
                    ))
                else:
                    orders.append(SpreadOrder(
                        legs=[(m1, "buy"), (m2, "sell")],
                        size=self.calculate_size(-edge),
                        max_cost=-edge * 0.5
                    ))

        return orders
```

#### Type D: Noise Trader
Random trading for liquidity.

```python
class NoiseTrader(Agent):
    def __init__(self, activity_rate: float, size_distribution):
        self.activity_rate = activity_rate
        self.size_dist = size_distribution

    def generate_orders(self, market_state, portfolio) -> List[Order]:
        if random.random() > self.activity_rate:
            return []  # Inactive this round

        # Pick random market
        market_id = random.choice(list(market_state.keys()))
        mid_price = market_state[market_id]

        # Random side and size
        side = random.choice(["buy", "sell"])
        size = self.size_dist.sample()

        # Price near mid with noise
        price = mid_price + random.gauss(0, 0.05)
        price = max(0.01, min(0.99, price))  # Clamp

        return [Order(market_id, side, size, price)]
```

### Effort Estimate
- LLM agent: 300 LOC + prompt engineering
- MM bot: 150 LOC
- Arb bot: 200 LOC
- Noise trader: 50 LOC
- Agent orchestration: 200 LOC
- **Total: ~900 LOC, 3-4 days**
- **LLM API costs**: ~$0.10-0.50 per simulated batch (depends on agent count)

---

## Component 3: Matching Engines

### Engine A: Batch Auction (Our System)

```python
class BatchAuctionEngine(MatchingEngine):
    def __init__(self, solvers: List[Solver]):
        self.solvers = solvers

    def run_batch(self, orders: List[Order]) -> BatchResult:
        # 1. Solve single-market
        base = self.solve_base(orders)

        # 2. Collect patches
        patches = []
        for solver in self.solvers:
            patches.extend(solver.propose_patches(orders, base))

        # 3. Select patches (MWIS)
        selected = self.select_patches(patches)

        # 4. Apply and validate
        solution = self.apply_patches(base, selected)

        return BatchResult(
            fills=solution.fills,
            prices=solution.prices,
            welfare=solution.welfare
        )
```

### Engine B: Continuous CLOB

```python
class CLOBEngine(MatchingEngine):
    def __init__(self):
        self.orderbooks = {}  # market_id -> OrderBook

    def process_order(self, order: Order) -> List[Fill]:
        book = self.orderbooks[order.market_id]
        fills = []

        remaining = order.size
        while remaining > 0:
            best = book.get_best_opposite(order.side)
            if best is None or not order.matches(best):
                break

            fill_size = min(remaining, best.size)
            fills.append(Fill(order.id, best.id, fill_size, best.price))
            remaining -= fill_size
            book.reduce(best.id, fill_size)

        if remaining > 0:
            book.add(order.with_size(remaining))

        return fills

    def run_batch(self, orders: List[Order]) -> BatchResult:
        """Simulate CLOB by processing orders in arrival order."""
        all_fills = []

        # Shuffle to simulate random arrival
        shuffled = random.shuffle(orders.copy())

        for order in shuffled:
            fills = self.process_order(order)
            all_fills.extend(fills)

        return BatchResult(fills=all_fills, ...)
```

### Engine C: Periodic CLOB (Hybrid)

```python
class PeriodicCLOBEngine(MatchingEngine):
    """CLOB but clears every N seconds instead of continuously."""

    def __init__(self, period_seconds: float):
        self.period = period_seconds
        self.clob = CLOBEngine()

    def run_batch(self, orders: List[Order]) -> BatchResult:
        # Accumulate orders, then clear
        for order in orders:
            self.clob.orderbooks[order.market_id].add(order)

        # Clear all at once (call auction style)
        return self.clob.clear_all_markets()
```

### Effort Estimate
- Batch engine: 400 LOC (uses solver code)
- CLOB engine: 300 LOC
- Periodic CLOB: 100 LOC
- Common interfaces: 100 LOC
- **Total: ~900 LOC, 3-4 days**

---

## Component 4: Metrics Collector

```python
@dataclass
class BatchMetrics:
    # Fill quality
    fill_rate: float           # Orders filled / orders submitted
    partial_fill_rate: float   # Average fill fraction

    # Price quality
    price_accuracy: float      # |clearing_price - true_probability|
    spread: float              # Bid-ask spread

    # Welfare
    total_welfare: float       # Sum of surpluses
    buyer_surplus: float
    seller_surplus: float
    mm_profit: float

    # Cross-market
    cross_market_fills: int
    cross_market_welfare: float

    # Solver performance
    base_welfare: float        # Single-market only
    patch_welfare: float       # Added by patches
    solve_time_ms: float
    num_patches_proposed: int
    num_patches_selected: int

class MetricsCollector:
    def compute_metrics(
        self,
        orders: List[Order],
        result: BatchResult,
        world: WorldState
    ) -> BatchMetrics:

        # Fill rate
        filled = sum(1 for o in orders if result.fill(o.id).amount > 0)
        fill_rate = filled / len(orders)

        # Price accuracy
        accuracies = []
        for market_id, price in result.prices.items():
            true_price = world.get_true_price(market_id, 0)
            accuracies.append(abs(price - true_price))
        price_accuracy = 1 - np.mean(accuracies)

        # Welfare
        total_welfare = sum(
            self.compute_surplus(order, result.fill(order.id), result.prices)
            for order in orders
        )

        # ... etc

        return BatchMetrics(...)
```

### Effort Estimate
- Metrics computation: 250 LOC
- Aggregation/reporting: 150 LOC
- Visualization: 200 LOC
- **Total: ~600 LOC, 2 days**

---

## Experiment Specifications

### Experiment 1: Cross-Market Value

**Question**: How much welfare comes from cross-market matching?

**Setup**:
- 100 markets, 5000 orders per batch
- Vary cross-market order percentage: 0%, 10%, 20%, 30%
- Run 100 batches per configuration

**Metrics**:
- Total welfare
- Welfare from cross-market orders specifically
- Fill rate for cross-market orders

**Expected insight**: Quantify the value proposition of cross-market support.

### Experiment 2: Patch Algorithm Quality

**Question**: How far from optimal is patch-based solving?

**Setup**:
- Small batches (100 orders, 10 markets) where we can solve optimally
- Compare: optimal ILP vs patch-based vs greedy

**Metrics**:
- Welfare ratio: patch_welfare / optimal_welfare
- Time ratio: patch_time / optimal_time

**Expected insight**: If patch achieves >90% of optimal, it's good enough.

### Experiment 3: Solver Specialization

**Question**: Do specialized solvers find different value?

**Setup**:
- Run with single generalist solver
- Run with 3 specialized solvers (spread, butterfly, JIT)
- Run with 5 specialized solvers

**Metrics**:
- Total welfare by configuration
- Patches accepted by solver type
- Overlap in patches proposed

**Expected insight**: Does specialization actually help?

### Experiment 4: Batch vs CLOB

**Question**: What's the tradeoff?

**Setup**:
- Same orderbook, three engines: Batch, CLOB, Periodic CLOB
- Vary: latency assumptions for CLOB

**Metrics**:
- Welfare
- Fill rate
- Price accuracy vs true probability
- Execution quality (slippage)

**Expected insight**: Quantify batch auction benefits/costs.

### Experiment 5: LLM Agent Realism

**Question**: Do LLM agents behave realistically?

**Setup**:
- Compare LLM agent orderflow patterns to:
  - Historical Polymarket data
  - Theoretical models (Kyle, Glosten-Milgrom)

**Metrics**:
- Order size distribution
- Price sensitivity
- Reaction to news
- Autocorrelation of trades

**Expected insight**: Can we trust LLM simulation?

---

## Effort Summary

| Component | LOC | Days | Dependencies |
|-----------|-----|------|--------------|
| World State | 550 | 1-2 | None |
| Agent Factory | 900 | 3-4 | LLM API |
| Matching Engines | 900 | 3-4 | Solver code |
| Metrics | 600 | 2 | None |
| Experiment Framework | 400 | 1-2 | All above |
| Analysis/Viz | 300 | 1 | Metrics |
| **Total** | **~3650** | **12-16 days** | |

### With AI Assistance

If using AI coding assistants:
- Core framework: 4-5 days
- Agent implementations: 2-3 days
- Experiments: 2-3 days
- Analysis: 1-2 days
- **Total: 9-13 days**

### Cost Estimates

**LLM API costs for simulation**:
- Per batch with 10 LLM agents: ~$0.10-0.30 (GPT-4) or ~$0.01-0.03 (GPT-3.5)
- 1000 batches with 10 agents: $100-300 (GPT-4) or $10-30 (GPT-3.5)
- Recommendation: Use GPT-3.5 for most runs, GPT-4 for validation

**Compute costs**:
- Mostly CPU-bound (LP solving, MWIS)
- Local machine sufficient for development
- For large-scale: 1000 batches × 5s = 1.5 hours

---

## Data Sources

### Option 1: Polymarket Historical Data

**Availability**:
- Public orderbook snapshots exist
- Trade history available via API
- ~2 years of data

**Challenge**:
- CLOB format, not batch
- Need to "batch-ify" by aggregating orders in time windows

**Approach**:
```python
def convert_polymarket_to_batches(trades, window_seconds=60):
    batches = []
    current_batch = []
    current_window_start = trades[0].timestamp

    for trade in trades:
        if trade.timestamp - current_window_start > window_seconds:
            batches.append(current_batch)
            current_batch = []
            current_window_start = trade.timestamp
        current_batch.append(trade_to_order(trade))

    return batches
```

### Option 2: Options Market Data

**Availability**:
- CBOE, CME have historical data
- Expensive ($$$)
- Very clean, high volume

**Value**:
- Real cross-instrument trading patterns
- Spread trades, butterflies, etc.
- Good for validating cross-market order types

### Option 3: Synthetic (Recommended for V1)

**Approach**:
- Define distributions for each order type
- Generate based on market conditions
- Validate against real data qualitatively

**Advantages**:
- Full control
- No data licensing issues
- Can test edge cases

```python
def generate_synthetic_batch(
    num_markets: int,
    num_orders: int,
    cross_market_pct: float,
    mm_count: int,
    retail_count: int,
    arb_count: int
) -> List[Order]:

    orders = []

    # Market maker orders (many markets, both sides)
    for _ in range(mm_count):
        mm = MarketMakerBot(spread=0.05, ...)
        orders.extend(mm.generate_orders(...))

    # Retail orders (few markets, one side)
    for _ in range(retail_count):
        market = random.choice(markets)
        side = random.choice(["buy", "sell"])
        size = np.random.lognormal(mean=2, sigma=1)
        price = market.mid + random.gauss(0, 0.03)
        orders.append(Order(market.id, side, size, price))

    # Arb orders (cross-market)
    for _ in range(arb_count):
        arb = ArbitrageBot(min_edge=0.02, ...)
        orders.extend(arb.generate_orders(...))

    return orders
```

---

## Questions to Answer Through Simulation

1. **What's the welfare gap?** Patch vs optimal
2. **What's the CLOB comparison?** Welfare, fill rate, price accuracy
3. **Do specialized solvers help?** Measure added welfare
4. **What batch size is optimal?** Trade-off curve
5. **How much cross-market value exists?** Justify complexity
6. **Is JIT worth it?** Compare with/without
7. **Are LLM agents realistic?** Validate against real data
