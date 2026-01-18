# Matching Simulation Documentation

This documentation covers the prediction market matching simulation system.

## Overview

The matching simulation system solves the NP-hard problem of optimal order matching in prediction markets with:
- Multiple correlated markets
- Bundle orders spanning multiple markets
- Liquidity constraints
- All-or-none (AON) fill requirements

## Components

- **matching-engine**: Core data structures and order book mechanics
- **matching-solver**: Multiple solver implementations (greedy, MILP, composite)
- **matching-scenarios**: Test scenario generators
- **matching-sim**: CLI tool for running simulations
- **jit-study**: JIT liquidity research tool

## Documentation

- [Architecture](architecture.md) - System design and component interactions
- [Solvers](solvers.md) - Solver algorithms and strategies
- [Scenarios](scenarios.md) - Available test scenarios
- [CLI Usage](cli.md) - Command-line interface guide

## Quick Start

```bash
# Run tests
just test

# Run a quick verification
cargo run --bin matching-sim --release -- --quick

# Compare all solvers on a scenario
cargo run --bin matching-sim --release -- --scenario presidential --solver all

# Run realistic scenario
just realistic-small
```

## Key Concepts

### Frequent Batch Auction (FBA)

Orders are collected over a time window and matched simultaneously at a uniform clearing price. This prevents front-running and ensures fair execution.

### Welfare Maximization

The solver maximizes total welfare: `sum((limit_price - fill_price) * fill_qty)` across all filled orders.

### Bundle Orders

Orders can span multiple markets with arbitrary payoff structures, enabling complex trading strategies like:
- "Buy YES on both A and B" (conjunction)
- "Buy YES on A OR YES on B" (disjunction)
- Arbitrage across correlated markets
