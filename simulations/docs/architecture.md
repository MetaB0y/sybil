# Architecture

## System Overview

```
+------------------+     +------------------+     +------------------+
|  matching-engine |---->| matching-solver  |---->|  matching-sim    |
+------------------+     +------------------+     +------------------+
        |                        |
        v                        v
+------------------+     +------------------+
|matching-scenarios|     |    jit-study     |
+------------------+     +------------------+
```

## Crate Dependencies

- `matching-engine` - Core types, no dependencies on other crates
- `matching-scenarios` - Depends on `matching-engine`
- `matching-solver` - Depends on `matching-engine`
- `matching-sim` - Depends on all above
- `jit-study` - Independent research tool

## matching-engine

Core data structures for the matching problem.

### Key Types

- `Problem` - Complete problem specification
- `MarketSet` - Collection of markets with outcomes
- `Order` - Trading order with payoffs and constraints
- `LiquidityPool` - Available liquidity per market/outcome
- `Fill` - Execution result for an order

### Order Book

Uses a simple price-time priority order book:
- `OrderBook` - Single market/outcome book
- `BookLevel` - Price level with available quantity

## matching-solver

Multiple solver implementations with a common `Solver` trait.

### Solver Hierarchy

```
Solver (trait)
├── GreedySolver           # O(n log n) heuristic
├── MultiHeuristicSolver   # Multiple sort strategies
├── RandomizedGreedySolver # Random order shuffling
├── MilpSolver             # Optimal via MILP
├── CompositeSolver        # Problem decomposition
└── SolverPlatform         # Production orchestrator
```

### SolverPlatform

The production-ready solver that:
1. Runs MILP with timeout
2. Runs greedy and heuristic solvers in parallel
3. Combines results via MWIS (Maximum Weight Independent Set)
4. Returns best non-conflicting fills

### Composition Module

Handles problem decomposition:
- `Decomposer` - Splits problem into independent clusters
- `SolutionMerger` - Combines partial solutions
- `PartialSolution` - Solution for a sub-problem

### Combiner Module

Platform-style solution combining:
- `SolutionCombiner` - Combines multiple complete solutions
- `ConflictGraph` - Tracks fill conflicts
- `MwisSolver` - Solves MWIS for best combination

## matching-scenarios

Test scenario generators for benchmarking.

### Categories

1. **Standard** - Presidential, tournament, random
2. **Complex** - Nested bundles, conditional chains
3. **Stress** - Mega scenarios, MILP killers
4. **Planted** - Known patterns for testing specific solvers
5. **Realistic** - Production-like order distributions

## matching-sim

CLI simulation harness.

### Structure

- `main.rs` - CLI parsing and orchestration
- `scenarios.rs` - Scenario name to generator mapping
- `runners.rs` - Specialized test runners
- `metrics.rs` - Performance metrics collection

## Data Flow

```
1. Problem Creation
   scenarios.rs::create_problem(name, seed)
   └── Returns Problem with markets, orders, liquidity

2. Solving
   solver.solve(&problem)
   └── Returns MatchingResult with fills, welfare

3. Metrics Collection
   OptimalityMetrics::from_result(...)
   └── Calculates fill rate, welfare gaps

4. Output
   comfy_table formatting
   └── Colored terminal output
```

## Configuration

### PlatformConfig

```rust
PlatformConfig {
    total_time_budget_ms: 5000,  // Total time allowed
    milp_time_fraction: 0.6,     // Fraction for MILP
    seed: 42,                    // Random seed
    include_arbitrage: true,     // Run arbitrage detector
    include_bundle_decomposer: true,
    include_chain_finder: true,
}
```

### MilpConfig

```rust
MilpConfig {
    time_limit_secs: Some(10.0), // MILP timeout
    gap_tolerance: 0.01,         // Optimality gap tolerance
}
```
