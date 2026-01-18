# Solvers

## Overview

The matching problem is NP-hard due to bundle orders and constraints. We provide multiple solver strategies with different speed/quality trade-offs.

## Solver Comparison

| Solver | Speed | Quality | Use Case |
|--------|-------|---------|----------|
| GreedySolver | O(n log n) | Good | Production baseline |
| MultiHeuristicSolver | O(k * n log n) | Better | Quick improvement |
| MilpSolver | Exponential | Optimal* | Benchmark, small problems |
| CompositeSolver | O(n log n) | Good+ | Complex constraints |
| SolverPlatform | Configurable | Best | Production |

*With sufficient time

## GreedySolver

Fast heuristic that processes orders by welfare potential.

### Algorithm

1. Sort orders by `limit_price * max_fill` (descending)
2. For each order, try to fill against available liquidity
3. Skip orders that can't be fully filled (AON) or lack liquidity

### Strengths
- Very fast: O(n log n)
- Deterministic results
- Good baseline performance

### Weaknesses
- Can miss optimal combinations
- Doesn't consider order interactions

## MultiHeuristicSolver

Tries multiple sorting strategies and returns the best result.

### Strategies

1. **Welfare** - Sort by limit * qty (descending)
2. **Price** - Sort by limit price (descending)
3. **Quantity** - Sort by max fill (descending)
4. **InverseWelfare** - Sort by limit * qty (ascending)
5. **PricePerUnit** - Sort by limit/qty ratio (descending)

### Usage

```rust
let solver = MultiHeuristicSolver::new();
let result = solver.solve(&problem);
```

## RandomizedGreedySolver

Shuffles order processing to escape local optima.

### Parameters
- `num_iterations` - Number of random shuffles (default: 10)
- `seed` - Random seed for reproducibility

## MilpSolver

Optimal solver using Mixed Integer Linear Programming.

### Formulation

Variables:
- `x[i]` - Binary, whether order i is filled
- `q[i]` - Continuous, quantity filled for order i

Objective:
```
maximize sum((limit[i] - cost[i]) * q[i])
```

Constraints:
- Liquidity: `sum(q[i] for orders using (m,o)) <= available[m,o]`
- AON: `q[i] = max_fill[i] * x[i]` if order is AON
- Partial: `q[i] <= max_fill[i] * x[i]` otherwise

### Usage

```rust
// With timeout
let solver = MilpSolver::with_timeout(10.0);
let result = solver.solve(&problem);

// With dual values (for analysis)
let (result, duals) = solver.solve_with_duals(&problem);
```

### SolveStatus

- `Optimal` - Proven optimal solution
- `Feasible` - Valid solution, may not be optimal
- `Infeasible` - No valid solution exists
- `Timeout` - Hit time limit

## CompositeSolver

Decomposes problems and routes to specialized solvers.

### Decomposition

1. Build market graph (markets connected by shared orders)
2. Find connected components (independent clusters)
3. Solve each cluster independently
4. Merge results

### Specialized Solvers

- **ArbitrageDetector** - Finds riskless profit from constraint mispricing
- **ChainFinder** - Exploits implication chains
- **BundleDecomposer** - Finds complementary bundle sets

## SolverPlatform

Production orchestrator combining all solvers.

### Algorithm

1. **Parallel Phase**
   - Start MILP solver with time budget
   - Run greedy, heuristic, specialized solvers

2. **Collection Phase**
   - Gather all complete solutions

3. **Combination Phase**
   - Build conflict graph
   - Solve MWIS to select best non-conflicting fills

4. **Return** best combined result

### Configuration

```rust
let config = PlatformConfig {
    total_time_budget_ms: 5000,
    milp_time_fraction: 0.6,  // 60% of time for MILP
    seed: 42,
    include_arbitrage: true,
    include_bundle_decomposer: true,
    include_chain_finder: true,
};

let platform = SolverPlatform::with_config(config);
let result = platform.solve(&problem);
```

### Result Analysis

```rust
let result = platform.solve(&problem);
result.print_summary();  // Detailed breakdown by solver
```

## Choosing a Solver

### Small Problems (< 100 orders)
Use `MilpSolver` for optimal results.

### Medium Problems (100-1000 orders)
Use `SolverPlatform` with 5-10s budget.

### Large Problems (> 1000 orders)
Use `SolverPlatform` with greedy fallback:
```rust
let config = PlatformConfig {
    total_time_budget_ms: 2000,
    milp_time_fraction: 0.3,  // Less time on MILP
    ..Default::default()
};
```

### Latency-Critical
Use `GreedySolver` for sub-millisecond response.
