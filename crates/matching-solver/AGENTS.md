# matching-solver

FBA (Frequent Batch Auction) solver for prediction markets.

## Purpose

Solves the matching problem: given orders and liquidity, find clearing prices and fills
that maximize welfare while respecting constraints.

## Architecture

The solver operates in phases via the `Pipeline`:

1. **Price Discovery** (`LocalSolver`): Find clearing prices per market
2. **Negrisk Arbitrage** (`NegriskSolver`): Exploit price inconsistencies
3. **MM Allocation** (`MmAllocator`): Respect market maker budget constraints
4. **Partial Solvers**: MILP and other optional solvers for alternative solutions

## Key Components

### LocalSolver (`local_solver.rs`)
Per-market price discovery. Finds where supply/demand curves cross.
This IS the FBA clearing logic for single markets.

### NegriskSolver (`specialized/negrisk.rs`)
Detects and exploits arbitrage when prices for mutually exclusive outcomes
don't sum to exactly $1. Creates welfare-adding fills instead of adjusting prices.

### MmAllocator (`mm_allocator.rs`)
Handles market maker budget constraints. Uses Lagrangian relaxation
to activate orders while respecting per-MM budgets.

### MilpSolver (`milp.rs`, feature-gated)
Optimal ILP formulation of the matching problem:
- Provably optimal welfare (given time budget)
- Feature-gated behind `milp` (requires HiGHS)

### GreedySolver (`greedy.rs`)
Simple heuristic that fills orders by welfare potential.
Used as fallback or for comparison.

### Pipeline (`pipeline.rs`)
Configurable pipeline combining the above components.
Use `Pipeline::with_negrisk()` for the recommended configuration.

## Usage

```rust
use matching_solver::Pipeline;

let pipeline = Pipeline::with_negrisk();
let result = pipeline.solve(&problem);
```

## Dependencies

- `matching-engine`: Core types

## Optional Features

- `milp`: Enable MILP solver for optimal (but slow) solutions
