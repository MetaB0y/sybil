# Sybil V2: Prediction Market Engine - Meta-Plan

## Overview

Sybil V2 is a prediction market exchange using Frequent Batch Auctions (FBA) with:
- **LP-based order representation** - orders are linear constraints, not simple limit orders
- **Solver network** - user-submitted WASM bots run in TEE to propose execution plans
- **Block builder** - combines solver solutions maximizing welfare + volume
- **Flash quoting** - user balance is a constraint in the LP, enabling capital-efficient cross-market trading

### Key Design Decisions (Confirmed)
1. **Single clearing price** per market per batch (classical FBA property)
2. **Solvers see sealed orderbook** after batch closes, have fixed window (~1s) to propose solutions
3. **LP determines prices** via solver competition, block builder reconciles
4. **Max exposure collateral model** - user balance is LP constraint
5. **Solver outputs full execution plans** - engine validates
6. **Integer arithmetic only** - u64 for amounts, i64 fixed-point for weights (scale factor TBD, e.g., 10^6)
7. **Hierarchical outcome IDs** - bit-packed u64 (market_id | outcome_index)
8. **Pluggable incentive system** - both fee-share and welfare components, designed as boundary
9. **Infeasibility handling** - solver infeasible = reject solution; builder infeasible = halt (critical bug)
10. **Scale factors** - PRICE_SCALE = WEIGHT_SCALE = 10^9 (9 decimals). Use u128 for intermediate calculations.
11. **Empty batch fallback** - Protocol runs greedy fallback bot (not in engine). If all fail, empty execution. Orders have expiry.
12. **Solver registration** - Solvers must register + stake. Future: on-chain liquidity pools (solver-as-fund-manager model)

---

## Phase 1: Specification Documents

Before any code, we need these specs. Each should be a standalone document that can be reviewed independently.

### 1.1 Core Data Model Spec (`docs/specs/data-model.md`)

**Purpose**: Define all primitive types and their invariants.

**Core Types** (Rust-style):
```rust
// Amounts in basis units (e.g., 1 unit = 0.001 sUSDS)
type Amount = u64;

// Fixed-point weight with scale factor WEIGHT_SCALE (e.g., 10^6)
type Weight = i64;  // Positive = long, Negative = short

// Bit-packed: upper 48 bits = market_id, lower 16 bits = outcome_index
// Supports 2^48 markets, 65536 outcomes per market
type OutcomeId = u64;

// Extract helpers
fn market_id(oid: OutcomeId) -> u64 { oid >> 16 }
fn outcome_index(oid: OutcomeId) -> u16 { (oid & 0xFFFF) as u16 }
fn make_outcome_id(market: u64, index: u16) -> OutcomeId { (market << 16) | (index as u64) }
```

**Invariants**:
- `INV-1`: For any market, sum of outcome prices = PRICE_SCALE (e.g., 10^6 = 100%)
- `INV-2`: For any user, sum of executed costs <= balance
- `INV-3`: Positions can be negative (short) but net position value + balance >= 0
- `INV-4`: All arithmetic checked for overflow

### 1.2 Order Specification (`docs/specs/order-spec.md`)

**Purpose**: Formally define the linear constraint order format.

**Core Types**:
```rust
struct ConstraintTerm {
    outcome_id: OutcomeId,
    weight: Weight,  // Fixed-point. Positive = buy/long, Negative = sell/short
}

struct LinearConstraint {
    terms: Vec<ConstraintTerm>,  // LHS: w1*x1 + w2*x2 + ...
    min_fill: Amount,            // RHS lower bound
    max_fill: Amount,            // RHS upper bound
}

struct Order {
    order_id: u64,
    trader_id: u64,
    constraints: Vec<LinearConstraint>,
    max_cost: Amount,            // Maximum willing to pay (soft cap at execution price)
    conditions: Vec<Condition>,  // Pre/post-solve conditions
    expiry_timestamp: u64,       // Unix millis — order expires after this time
    nonce: u64,                  // For replay protection
}

enum Condition {
    MaxAveragePrice(Amount),     // Post-solve: avg price <= limit
    MinFillFraction(u32),        // Post-solve: fill >= fraction (basis points)
    ExternalOracle {             // Pre-solve: ENGINE evaluates at batch seal
        oracle_id: u64,
        operator: Operator,
        value: Amount,
    },
}
```

**Condition Evaluation Timing** (critical for determinism):
- `ExternalOracle`: Engine evaluates at batch seal, BEFORE creating SolverInput
  - Orders failing oracle check are marked "Inactive" and excluded from SolverInput
  - All solvers see identical active order set (determinism)
- `MaxAveragePrice`, `MinFillFraction`: Engine validates AFTER solver submits plan
  - If condition fails, fill is set to 0 for that order

```rust
```

**max_cost Semantics** (Ex-Post Budget Check for Flash Liquidity):
- `max_cost` is a **Soft Cap** based on EXECUTION price, not limit price
- Constraint: `Σ (fill[o] * clearing_price[o]) <= max_cost`
- This enables leverage within a batch: bid $1000 on 10 markets, only pay for what clears cheaply
- Validation is O(N): iterate fills, multiply by finalized prices, sum

**Invariants**:
- `ORD-1`: At least one constraint per order
- `ORD-2`: min_fill <= max_fill for all constraints
- `ORD-3`: max_cost > 0
- `ORD-4`: All referenced outcome_ids must exist
- `ORD-5`: expiry_timestamp > current_time at submission

**Example Encodings** (to be detailed in spec):
- Simple limit buy: single constraint `[(outcome, +1)] in [amount, amount]`
- Jumbo (A & B): multiple constraints that must all be satisfied
- Spread trade: `[(A, +1), (B, -1)] in [min, max]` — long A, short B
- Distribution bet: N constraints covering N bins with weights from PDF

### 1.3 LP Formulation Spec (`docs/specs/lp-formulation.md`)

**Purpose**: The mathematical heart of the system.

**Decision Variables**:
```
fill[o] ∈ [0, 1]           for each order o     (fill fraction)
price[i] ∈ [0, PRICE_SCALE] for each outcome i  (clearing price)
```

**Constraints**:
```
// C1: Market probability constraint
∀ market m: Σ price[i] for i ∈ m = PRICE_SCALE

// C2: User budget constraint
∀ user u: Σ fill[o] * cost(o, price) for o ∈ orders(u) <= balance[u]

// C3: Order linear constraints
∀ order o, constraint c in o.constraints:
    c.min_fill <= fill[o] * Σ(c.terms[j].weight * shares[j]) <= c.max_fill

// C4: Order price limit
∀ order o: execution_price(o, price) <= o.max_cost OR fill[o] = 0
```

**Objective Function** (LINEAR — this is important):
```
maximize: α * welfare + β * volume

where:
  welfare = Σ fill[o] * (bid_limit[o] - ask_limit[o])  // Clearing prices CANCEL OUT
  volume  = Σ fill[o] * notional_at_limit[o]          // Use LIMIT price, not clearing price
```

**Critical Insight — Linearity Analysis:**
- **Objective is LINEAR**: Both welfare and volume are `Σ fill * constant`
  - Welfare: The clearing price terms cancel out mathematically
  - Volume: Defined as notional at LIMIT price (constant), not clearing price
- **Budget Constraint is BILINEAR**: `Σ fill[o] * clearing_price[o] <= balance`
  - Both fill and price are unknowns → product of two variables

**Implication for Solvers:**
- Engine validation is O(N) — just multiply fills by known prices and sum
- Solvers cannot use simple Simplex in one pass
- Solvers must use **Fixed-Point Iteration**: Guess price → Solve linear fills → Update price → Repeat

**Tolerance Epsilon** (to avoid dust rejection loops):
```
DUST_EPSILON = 10  // basis units
```
If computed cost exceeds balance by < DUST_EPSILON, clip the fill down rather than reject.

**Price Normalization** (for ring trades):
- In pure barter rings (A→B→C→A), infinite price vectors may be valid
- Rule: If ring has no numeraire, solver must provide consistent prices
- Engine validates: `sum(prices) = SCALE` per market (this anchors relative prices)

**Properties**:
- `PROP-1`: Uniform price — all fills in same market at same price
- `PROP-2`: No frontrunning — batch ordering doesn't affect outcome
- `PROP-3`: Incentive compatible — truthful bidding is optimal (under conditions TBD)

### 1.4 Batch Auction Protocol Spec (`docs/specs/fba-protocol.md`)

**Purpose**: Define the FBA lifecycle and timing.

**Contents**:
1. **Batch States**: `Accumulating` -> `Sealed` -> `Solving` -> `Executed`
2. **Timing Parameters**: batch_duration, solve_window, etc.
3. **State Transitions**: What triggers each transition
4. **Order Lifecycle**: submission -> validation -> inclusion -> execution/expiry
5. **Failure Modes**: What if no solver produces valid solution?

### 1.5 Solver Interface Spec (`docs/specs/solver-interface.md`)

**Purpose**: Define the contract between engine and solvers. This is the critical boundary.

**Solver Input** (provided by engine after batch seals):
```rust
struct SolverInput {
    batch_id: u64,
    orders: Vec<Order>,                        // All valid orders in batch
    positions: HashMap<(UserId, OutcomeId), i64>,  // Current positions (signed)
    balances: HashMap<UserId, Amount>,         // Available balances
    markets: Vec<MarketInfo>,                  // Market metadata
    prev_prices: HashMap<OutcomeId, Amount>,   // Previous batch clearing prices
}

struct MarketInfo {
    market_id: u64,
    outcomes: Vec<OutcomeId>,
    // No outcome names/metadata — solvers don't need it
}
```

**Solver Output**:
```rust
struct ExecutionPlan {
    batch_id: u64,                             // Must match input
    fills: Vec<OrderFill>,
    prices: Vec<(OutcomeId, Amount)>,          // Clearing prices
    jit_orders: Vec<Order>,                    // Solver-injected liquidity
}

struct OrderFill {
    order_id: u64,
    fill_fraction: u32,  // Basis points (0-10000 = 0%-100%)
}
```

**Order Classification**:
```rust
enum OrderSource {
    External,  // Signed by user, submitted during accumulation (0-59s)
    JIT,       // Signed by solver, submitted with ExecutionPlan
}
```

**Anti-Penny-Jumping: The Fee Moat**

Problem: Solver sees Bob's Sell @ $0.50, injects JIT Sell @ $0.499999999.
Result: Bob gets 0% fill, Solver captures spread with epsilon improvement.

Solution: **Asymmetric Fees**
- External Orders: 0 bps fee (or rebate)
- JIT Orders: 5 bps fee (JIT_FEE_BPS = 50)

This creates a "moat" — Solver must improve price by >5 bps to profitably displace.

**Tie-Breaker**: If price(JIT) == price(External), External gets 100% priority.

**Validation** (engine MUST check all):
- `VAL-1`: batch_id matches
- `VAL-2`: All order_ids exist in input
- `VAL-3`: All fill_fractions in [0, 10000]
- `VAL-4`: Prices sum to PRICE_SCALE per market
- `VAL-5`: Each filled order's constraints satisfied
- `VAL-6`: Each user's budget constraint satisfied
- `VAL-7`: JIT orders are valid and solver has balance for them
- `VAL-8`: JIT fee deducted from solver settlement balance

**WASM Sandbox Limits**:
- Memory: 256 MB max
- CPU: solve_window_ms (e.g., 1000ms)
- No host functions except: `log(msg)`, `get_time_remaining()`
- No network, filesystem, randomness

### 1.6 Block Builder Spec (`docs/specs/block-builder.md`)

**Purpose**: How to combine multiple solver solutions into canonical execution.

**Input**: `Vec<ExecutionPlan>` from all solvers that submitted in time

**Conflict Types**:
```rust
enum Conflict {
    PriceConflict {        // Same outcome, different prices
        outcome_id: OutcomeId,
        prices: Vec<Amount>,
    },
    FillConflict {         // Same order, different fill fractions
        order_id: u64,
        fills: Vec<u32>,
    },
    BudgetConflict {       // Combined fills exceed user balance
        user_id: u64,
        total_cost: Amount,
        balance: Amount,
    },
}
```

**Combination Algorithm** — Maximum Weight Independent Set (MWIS):

The key insight: plans that touch the same order are **conflicting** and cannot both be included.
This is a graph problem, not an arithmetic one.

```
1. Build conflict graph:
   - Node = (solver_id, order_id, fill, price)
   - Edge = connects nodes that touch the same order_id
   - Weight = contribution to (welfare + fee_rate * volume)

2. Solve MWIS:
   - Find maximum-weight subset where no two nodes share an edge
   - This is NP-hard in general, but greedy works well in practice

3. Greedy MWIS:
   a. Sort nodes by weight descending
   b. For each node in order:
      If no conflict with selected set: add to selected
   c. Return selected set

4. Verify combined solution:
   - Check all invariants on the combined fills
```

**Why Disjoint Sets (not partial merge):**
- If Solver A fills Order #101 at 50% and Solver B fills it at 50%, we pick ONE
- No arithmetic merging (100% fill) — too complex, edge cases with prices

**Scoring Function** (must align incentives):
```
score = welfare + (fee_rate * volume_at_limit_price)
```
- Volume defined at LIMIT price (not clearing price) to preserve linearity
- This ensures solvers are rewarded for finding volume without hurting welfare

**Invariants**:
- `BUILD-1`: If any valid plan was submitted, output is non-empty
- `BUILD-2`: Output passes all validation rules (VAL-1 through VAL-7)
- `BUILD-3`: Builder never produces infeasible output (halt if it would)
- `BUILD-4`: Same order never filled by multiple solvers (disjoint sets)

### 1.7 Sequencer Spec (`docs/specs/sequencer.md`)

**Purpose**: Transaction ordering and batch formation.

**Contents**:
1. **Transaction Types**: SubmitOrder, CancelOrder, Deposit, Withdraw
2. **Ordering Guarantees**: Within batch, order doesn't matter (FBA property)
3. **Batch Boundaries**: Fixed time interval (e.g., 12s like Ethereum)
4. **TEE Integration Points**: Where attestation is needed
5. **Future: ZK Proof Points**: What state transitions need proving

### 1.8 Solver Registry Spec (`docs/specs/solver-registry.md`)

**Purpose**: Solver registration, staking, and incentives boundary.

**Core Types**:
```rust
struct Solver {
    solver_id: u64,
    owner: Address,              // Who controls the solver
    wasm_hash: [u8; 32],         // Hash of WASM binary
    stake: Amount,               // Staked collateral
    accumulated_rewards: Amount,
    status: SolverStatus,
}

enum SolverStatus {
    Active,
    Suspended,      // Temporarily disabled (e.g., for misbehavior)
    Withdrawing,    // Stake unlock in progress
}
```

**Invariants**:
- `REG-1`: Only Active solvers can submit ExecutionPlans
- `REG-2`: JIT orders backed by solver's balance (can't exceed available)
- `REG-3`: Stake locked during Withdrawing period (e.g., 7 days)
- `REG-4`: JIT collateral locked until batch settles (prevents double-spend across batches)

**Stake vs Balance** (TBD but likely):
- `stake`: Locked collateral for registration, slashing on misbehavior
- `balance`: Available funds for JIT orders, receives rewards
- May be unified or separate — design types to support either

**Future Boundary** (not for V1, but design for it):
- On-chain liquidity pools that fund solvers
- Profit sharing between solver operator and LPs
- Solver performance metrics (fill rate, welfare generated)

---

## Phase 2: Architecture Documents

### 2.1 System Architecture (`docs/architecture/system-overview.md`)

**Purpose**: High-level component diagram and data flow.

**Components**:
```
┌─────────────────────────────────────────────────────────────┐
│                         TEE Boundary                         │
│  ┌─────────┐    ┌─────────┐    ┌──────────┐    ┌─────────┐  │
│  │Sequencer│───▶│  Engine │───▶│  Solver  │───▶│  Block  │  │
│  │         │    │         │    │  Runner  │    │ Builder │  │
│  └─────────┘    └─────────┘    └──────────┘    └─────────┘  │
│       │              │              │               │        │
│       ▼              ▼              ▼               ▼        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                    State Store                       │    │
│  │  (Markets, Orders, Positions, Balances, Batches)    │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Module Boundaries (`docs/architecture/module-boundaries.md`)

**Purpose**: Define crate/module structure with clear interfaces.

```
sybil/
├── crates/
│   ├── types/           # Core data types, no logic
│   ├── engine/          # State machine, validation, execution
│   ├── solver-api/      # Solver interface types (shared with WASM)
│   ├── solver-runner/   # WASM sandbox, solver orchestration
│   ├── block-builder/   # Solution combination logic
│   ├── sequencer/       # Transaction ordering, batch formation
│   ├── lp-utils/        # LP constraint helpers, validation
│   └── sim/             # Simulation harness, test utilities
├── solvers/             # Example solver implementations
│   ├── naive/           # Simple greedy solver
│   ├── lp-solver/       # Full LP solver (using HiGHS or similar)
│   └── arb-finder/      # Arbitrage cycle detection
└── tests/
    ├── property/        # Property-based tests
    ├── metamorphic/     # Metamorphic tests
    └── backtest/        # Historical data replay
```

### 2.3 Integration Points (`docs/architecture/integration-points.md`)

**Purpose**: Define boundaries for future components.

1. **ZK Proof Integration**
   - What state transitions need proving
   - Proof-friendly data structures
   - Verifier interface

2. **Smart Contract Integration**
   - Deposit/withdrawal bridge
   - Settlement finality
   - Dispute resolution

3. **External Oracle Integration**
   - Oracle data ingestion
   - Condition evaluation
   - Market resolution

### 2.4 Oracle Architecture (`docs/architecture/oracles.md`)

**Two Types of Information Feeds:**

**A. Public Feeds ("Common Knowledge")**
- What: Chainlink prices, election results, weather data
- How: Sequencer pushes into TEE for every batch
- Visibility: All solvers see identical data
- Use: Resolving markets, evaluating `ExternalOracle` conditions

**B. Private Feeds ("Secret Sauce")**
- What: Proprietary alpha (satellite data, OTC desk flow, etc.)
- Problem: If public, your edge is gone
- Solution: **Encrypted Context**

```rust
struct SolverRequest {
    execution_plan: ExecutionPlan,
    encrypted_context: Option<Vec<u8>>,  // Encrypted with TEE public key
}
```

- Mechanism:
  1. Solver encrypts secret data with TEE's public key
  2. Submits `encrypted_context` alongside solution
  3. Inside enclave: TEE decrypts, only THIS solver's WASM instance sees it
  4. Result: Trade executes based on signal, but signal never leaves TEE

- The public sees the *result* (Buy Order), never the *signal* ("Oil Low")

---

## Phase 3: Testing Strategy

### 3.1 Property Tests (`docs/testing/property-tests.md`)

Properties to verify:
- **Solvency**: No user ends up with negative balance
- **Market Constraint**: Outcome prices sum to 1
- **Order Constraint Satisfaction**: Filled orders respect their constraints
- **Budget Constraint**: User spending <= balance
- **Idempotency**: Same input -> same output
- **Determinism**: No randomness in matching

### 3.2 Metamorphic Tests (`docs/testing/metamorphic-tests.md`)

Relationships to verify:
- Adding a worse order doesn't change other fills
- Splitting an order shouldn't change total fill
- Order of solver solutions shouldn't matter (commutativity)
- Scaling all amounts by k scales all fills by k

### 3.3 Simulation Framework (`docs/testing/simulation.md`)

- Synthetic order generation (various distributions)
- Multi-agent simulation (competing strategies)
- Stress testing (many orders, many markets)
- Adversarial testing (try to break invariants)

---

## Phase 4: Implementation Plan

### Milestone 1: Core Types & Validation
- [ ] Define all types in `types` crate
- [ ] Order validation logic
- [ ] LP constraint construction helpers
- [ ] Unit tests for validation

### Milestone 2: Engine State Machine
- [ ] State store (in-memory)
- [ ] Batch lifecycle management
- [ ] Order submission/cancellation
- [ ] Position/balance tracking

### Milestone 3: Naive Solver
- [ ] Solver API definition
- [ ] Simple greedy solver (no LP, just match compatible orders)
- [ ] Engine validates solver output

### Milestone 4: LP Solver
- [ ] Integrate LP library (HiGHS recommended)
- [ ] Construct LP from orderbook
- [ ] Extract prices from dual variables
- [ ] Property tests for LP correctness

### Milestone 5: Block Builder
- [ ] Conflict detection
- [ ] Solution combination
- [ ] Welfare scoring

### Milestone 6: WASM Sandbox
- [ ] wasmtime integration
- [ ] Host function interface
- [ ] Resource limits

### Milestone 7: Sequencer
- [ ] Transaction types
- [ ] Batch formation
- [ ] Full integration test

### Milestone 8: Simulation & Backtesting
- [ ] Simulation harness
- [ ] Order generators
- [ ] Metrics collection

---

## Open Questions (Remaining)

### Resolved:
- ~~Precision~~ → u64 amounts, i64 fixed-point weights
- ~~Outcome IDs~~ → Hierarchical bit-packed u64
- ~~Solver incentives~~ → Pluggable system (boundary), both fees + welfare
- ~~LP infeasibility~~ → Solver infeasible = reject; Builder infeasible = halt
- ~~Spec format~~ → Type definitions + invariants

### Resolved (from review):

1. **Order ID generation**: Sequential u64 from sequencer ✓

2. **Price bounds**: Allow [0, SCALE]. Zero = worthless outcome. ✓

3. **Multi-collateral**: Single collateral (sUSDS) forever. Not multi-asset. ✓

4. **JIT order limits**: Limited by solver's available balance (can't go into debt). ✓

5. **Order expiry granularity**: **Timestamp** (not batch number)
   - Rationale: More robust to batch duration changes
   - TEE time is trusted within the enclave
   - `expiry_timestamp: u64` (Unix millis)

---

## Recommended Execution Order

### Step 1: Specs (Before Any Code)
1. `docs/specs/data-model.md` — Core types and invariants
2. `docs/specs/order-spec.md` — Order format with examples
3. `docs/specs/lp-formulation.md` — Mathematical foundation
4. `docs/specs/solver-interface.md` — The critical boundary
5. `docs/specs/block-builder.md` — Solution combination
6. `docs/specs/fba-protocol.md` — Batch lifecycle
7. `docs/specs/sequencer.md` — Transaction ordering
8. `docs/specs/solver-registry.md` — Registration/staking

### Step 2: Types Crate
- Implement all types from specs
- No logic, just data structures
- Derive serde, Clone, Debug, PartialEq
- Add validation functions as associated methods

### Step 3: Minimal Vertical Slice
Build a minimal working system end-to-end:
- Simple in-memory state store
- Hardcoded single solver (greedy, no LP)
- No block builder (single solver = no conflicts)
- Basic sequencer (manual batch triggers)
- Property tests for core invariants

### Step 4: Add Complexity Incrementally
- LP solver integration (HiGHS)
- Block builder (conflict resolution)
- WASM sandbox
- Multiple concurrent solvers
- Full sequencer with timing

---

## Verification Strategy

### Unit Tests
- Order validation (valid/invalid cases)
- Constraint satisfaction checking
- Price constraint verification
- Overflow handling in arithmetic

### Property Tests
- Solvency invariant: `∀ user: balance >= 0 after any execution`
- Market constraint: `∀ market: sum(prices) == SCALE`
- Fill bounds: `∀ order: 0 <= fill <= 1`
- Determinism: same input → same output

### Metamorphic Tests
- Adding dominated order doesn't change others' fills
- Scaling amounts scales fills proportionally
- Permuting solver submission order doesn't affect result

### Integration Tests
- Full batch lifecycle: submit → seal → solve → execute
- Multi-market scenarios
- Jumbo order execution
- Flash quoting (multi-market fills with shared balance)

### Simulation
- Synthetic order generation
- Stress test with 1000+ orders, 100+ markets
- Adversarial solver behavior
- **Budget Exhaustion Testing**: Verify engine correctly rejects solutions where price shift pushes user over budget by dust amount (bilinear variance edge case)

---

## Document Standards

Each spec should have:
- **Status**: Draft / Review / Approved
- **Last Updated**: Date
- **Types Section**: Rust-style type definitions
- **Invariants Section**: Numbered invariants (e.g., INV-1, ORD-1)
- **Examples Section**: Concrete worked examples
- **Open Questions**: Any unresolved decisions
