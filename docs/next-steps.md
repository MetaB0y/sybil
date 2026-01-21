# Next Steps

## Current State

### Implemented

#### matching-engine (Core Types)
- Order representation with linear constraints
- Fill execution and welfare calculation
- Liquidity pools (order books)
- Market definitions (binary and multi-outcome)
- MM constraints with budget tracking

#### matching-solver (Solving Algorithms)
- **LocalSolver** - Per-market clearing with price normalization
- **MmAllocator** - MM budget allocation via Lagrangian relaxation
- **PriceProjector** - Cross-market price consistency
- **Combiner** - MWIS-based solution combination
- **Pipeline** - Orchestration of all solver phases
- **Specialized solvers**: Arbitrage

#### matching-scenarios (Test Scenarios)
- Mega scenario generator (configurable scale)
- Stress test scenarios

### Not Yet Implemented

1. **Flash Liquidity**
   - MM provides conditional liquidity
   - Capital usage determined at clearing time

2. **External Solver Interface**
   - API for external solver submissions

---

## Priority Roadmap

### P0: MM Constraint Integration

1. **Verify MM Allocator Correctness**
   - Test fixed-point iteration with overlapping MMs
   - Add stats reporting (utilization, convergence)
   - Sanity check against simple heuristics

2. **Determine Solver Ordering**
   - Current: Per-market first, then MM allocation
   - Question: Should MM volume affect clearing prices?
   - May need iteration between phases

3. **Improve Test Coverage**
   - Multi-MM scenarios with overlap
   - Tight budget scenarios
   - Property tests for monotonicity

### P1: Cross-Market Patches

1. **Integrate Combiner with LocalSolver**
   - Cross-market orders create patches
   - MWIS selects non-conflicting improvements

2. **Bundle Order Support**
   - Multi-market atomic orders
   - Decomposition strategies

### P2: Production-Ready

1. **Deterministic Execution**
   - Identical inputs → identical outputs
   - No hidden randomness

2. **Performance Optimization**
   - Profile at 50K orders
   - Parallel per-market solving

3. **Error Handling**
   - Graceful degradation
   - Fallback strategies

---

## Technical Debt

- [ ] Clean up unused solver code
- [ ] Property-based tests for all solvers
- [ ] Benchmark suite with regression tracking
- [ ] Documentation for public APIs

---

## Research Questions

### MM Constraints
1. How tight are budgets in practice?
2. How often does fixed-point iterate >1 time?
3. What's the welfare gain from MM participation?

### Cross-Market
1. How much welfare comes from cross-market matching?
2. How sparse is the conflict graph?
3. Does MWIS outperform greedy significantly?

---

## Decision Points

### Solver Ordering
**Question**: Should MM constraints be solved before or after per-market clearing?

**Options**:
1. After: Use prices from clearing, allocate MM budgets
2. Before: Let MM volume affect prices (needs iteration)
3. Interleaved: Fixed-point between clearing and allocation

**Current**: Option 1 (after), with fixed-point for multi-MM.

### Flash Liquidity
**Question**: How to handle MM liquidity that depends on clearing price?

**Options**:
1. Conservative: Use worst-case price for capital
2. Iterative: Solve, check budgets, re-solve
3. Dual: Lagrangian relaxation with price-dependent costs

**Recommendation**: Start with option 1, add iteration if needed.
