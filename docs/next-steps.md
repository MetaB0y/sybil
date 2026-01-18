# Next Steps

## Current State

### What's Implemented

#### matching-engine (Core Types)
- Order representation with linear constraints
- Fill execution and welfare calculation
- Liquidity pools (order books with bids/asks)
- Market definitions (binary and multi-outcome)
- State space enumeration for multi-market orders
- Problem structure combining orders, markets, liquidity, constraints

#### matching-solver (Solving Algorithms)
- **GreedySolver** - O(n log n) baseline solver
- **MilpSolver** - MILP formulation using HiGHS
- **RandomizedGreedySolver** - Multi-iteration randomized greedy
- **SolverPlatform** - Orchestrates multiple specialized solvers
- **SolutionCombiner** - MWIS-based combination of solver outputs
- **Specialized solvers**: Arbitrage, BundleDecomposer, ChainFinder

#### matching-scenarios (Test Scenarios)
- Presidential election scenario
- Realistic multi-market scenarios (various sizes)
- Stress test scenarios
- Random order generation

#### matching-sim (CLI Tool)
- Runs scenarios with configurable solvers
- Reports fill rates, welfare, timing
- Supports all solver types

#### jit-study (Research)
- JIT welfare impact analysis
- Experimental simulations

### What's NOT Implemented

1. **Batch Auction Protocol**
   - No actual batching/timing
   - No order accumulation period
   - No sealed orderbook state machine

2. **External Solver Interface**
   - Solvers run in-process only
   - No TLS/API for external submissions
   - No WASM sandbox

3. **JIT in Production**
   - JIT mechanism designed but not integrated
   - No fee collection
   - No rebate distribution

4. **User Balances/Collateral**
   - No budget constraint enforcement
   - No flash quoting
   - No position tracking

5. **Persistence**
   - Everything in-memory
   - No database/state storage

6. **Network/Deployment**
   - No TEE integration
   - No sequencer
   - No API server

---

## Priority Roadmap

### P0: Production-Ready Matching

**Goal**: Matching engine that can be deployed with real orders.

1. **Budget Constraint Enforcement**
   - Add user balance tracking to Problem
   - Enforce `sum(fills × prices) <= balance` in validation
   - Handle partial fills due to budget exhaustion

2. **Deterministic Execution**
   - Ensure identical inputs produce identical outputs
   - Remove any remaining randomness in production path
   - Add comprehensive property tests

3. **Performance Optimization**
   - Profile hot paths
   - Optimize LP construction
   - Benchmark at target scale (10K orders)

4. **Error Handling**
   - Graceful degradation on solver failures
   - Fallback to greedy on timeouts
   - Proper error types and propagation

### P1: Batch Protocol

**Goal**: Full FBA lifecycle with batching.

1. **Batch State Machine**
   ```
   Accumulating -> Sealed -> Solving -> Executed
   ```
   - Define state transitions
   - Handle timing
   - Order lifecycle (submit, cancel, expire)

2. **Solver Interface**
   - Define `SolverInput` / `ExecutionPlan` types
   - Validation of solver outputs
   - Timeout handling

3. **Solution Validation**
   - All fills satisfy order constraints
   - All prices satisfy market constraints
   - No budget violations
   - Deterministic validation

### P2: JIT Integration

**Goal**: Market makers can provide JIT liquidity.

1. **JIT Submission Window**
   - After batch seals, before solving completes
   - Accept JIT order submissions

2. **Welfare Requirement**
   - JIT must improve total welfare
   - Minimum improvement threshold

3. **Fee Mechanism**
   - EIP-1559 style dynamic fees
   - Fee collection and distribution
   - Rebates to affected users

4. **Anti-Gaming**
   - Asymmetric fees (JIT vs regular orders)
   - Priority rules for ties

### P3: External Solvers

**Goal**: Third parties can run solvers.

1. **API Design**
   - Solver registration
   - Solution submission
   - Result notification

2. **Solver Isolation**
   - Solutions validated, not trusted
   - Rate limiting
   - (Optional) WASM sandbox

3. **Incentive Mechanism**
   - Fee sharing
   - Staking/slashing
   - Performance tracking

### P4: Full Protocol

**Goal**: Complete system deployment.

1. **TEE Integration**
   - Orderbook confidentiality
   - Solver execution isolation
   - Attestation

2. **Persistence Layer**
   - State storage
   - Order history
   - Audit trail

3. **Network Protocol**
   - Order submission API
   - WebSocket for updates
   - Block/batch publication

---

## Technical Debt

### Code Quality
- [ ] Add more unit tests for edge cases
- [ ] Property-based tests for solver correctness
- [ ] Benchmark suite with regression tracking
- [ ] Documentation for public APIs

### Architecture
- [ ] Separate validation from solving
- [ ] Make solvers stateless
- [ ] Define clear module boundaries
- [ ] Consider async for solver parallelism

### Base Solution Quality
- [ ] **Optimal single-market clearing** - Currently greedy; should solve each market as LP
  - For multi-outcome markets: ensure probabilities sum to 100%
  - Find true equilibrium price per market
  - Maximize single-market welfare before cross-market patches

  **Note**: This won't hurt cross-market solvers because:
  1. Patches can still adjust prices (via `price_adjustments`)
  2. Cross-market value comes from correlations, not single-market mispricing
  3. Better baseline = more accurate welfare delta calculations
  4. Currently single-market may have arbitrage opportunities that patches "fix" wastefully

### Cleanup
- [x] Remove unused composition module (-2.6k lines)
- [x] Remove unused specialized/conditional solver
- [ ] Consolidate duplicate scenario configs
- [ ] Clean up test helpers

---

## Research Questions

### Matching Quality
1. How far from optimal is patch-based solving?
2. What's the welfare gain from specialized solvers?
3. How does solution quality scale with order count?

### JIT Economics
1. What's the optimal JIT fee rate?
2. How much JIT volume is "too much"?
3. Do rebates make displaced users whole?

### Scaling
1. What's the practical limit on cross-market orders?
2. How does MWIS scale with patch count?
3. Can we shard by market cluster?

### Comparison
1. Batch auction vs CLOB: welfare comparison
2. Fill rate comparison
3. Price accuracy comparison

---

## Experiment Priorities

### Experiment 1: Cross-Market Value
Measure how much welfare comes from cross-market matching.
- Vary cross-market order percentage
- Compare with single-market only

### Experiment 2: Solver Specialization
Measure individual solver contributions.
- Run platform with/without each specialized solver
- Track fills contributed by each

### Experiment 3: JIT Impact
Simulate JIT liquidity provision.
- Welfare with/without JIT
- Fill rate improvement
- Price accuracy

### Experiment 4: Scale Stress Test
Find breaking points.
- Orders per batch
- Markets per batch
- Cross-market order density

---

## Milestones

| Milestone | Description | Estimated Effort |
|-----------|-------------|------------------|
| M1 | Budget constraints + validation | 1-2 weeks |
| M2 | Batch state machine | 1-2 weeks |
| M3 | JIT integration | 2-3 weeks |
| M4 | External solver API | 2-3 weeks |
| M5 | Persistence layer | 1-2 weeks |
| M6 | Network protocol | 2-3 weeks |
| M7 | TEE integration | 3-4 weeks |

Total to production-ready: ~3-4 months of focused work.

---

## Decision Points

### Flash Quoting
**Question**: Support bilinear budget constraints (budget at execution price)?

**Options**:
1. Linear only (budget at limit price) - simpler
2. Bilinear with iteration - more capital efficient, complex

**Recommendation**: Start linear, add bilinear as opt-in later.

### Solver Model
**Question**: Internal-only vs external solvers?

**Options**:
1. Internal only (trusted, in-process)
2. External via API (validated outputs)
3. External via WASM (sandboxed execution)

**Recommendation**: Start internal, add external API, defer WASM.

### Block Building
**Question**: Winner-takes-all vs MWIS combination?

**Options**:
1. Winner-takes-all (best single solution wins)
2. MWIS combination (combine non-conflicting patches)

**Current**: MWIS is implemented. Monitor if combination adds value vs complexity.
