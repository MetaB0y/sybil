# Sybil V2

A prediction market matching engine using Frequent Batch Auctions (FBA) with cross-market order support.

## Features

- **Linear constraint orders** - Orders expressed as LP constraints, supporting complex multi-market strategies
- **Batch auction matching** - Uniform clearing price prevents front-running
- **Multiple solvers** - Greedy, MILP, and randomized algorithms
- **Cross-market orders** - Bundles, spreads, conditionals across correlated markets
- **Solution combination** - MWIS-based combination of solver outputs

## Quick Start

```bash
# Run tests
just test

# Quick verification
cargo run --release --bin matching-sim -- --quick

# Compare all solvers on a scenario
cargo run --release --bin matching-sim -- --scenario presidential --solver all

# Run realistic scenario
just realistic-small
```

## Project Structure

```
sybil/
├── crates/
│   ├── matching-engine/     # Core types, orders, fills, liquidity
│   ├── matching-solver/     # Solver algorithms (greedy, MILP, platform)
│   ├── matching-scenarios/  # Test scenario generators
│   ├── matching-sim/        # CLI simulation tool
│   └── jit-study/           # JIT liquidity research
├── docs/                    # Documentation
└── justfile                 # Build/test commands
```

## Documentation

- [Architecture](docs/architecture.md) - System design and key decisions
- [Matching Algorithm](docs/matching-algorithm.md) - Patch-based cross-market solving
- [Order Types](docs/order-types.md) - Supported order types
- [JIT Design](docs/jit-design.md) - Just-in-time liquidity mechanism
- [CLI Usage](docs/cli.md) - Command-line interface
- [Scenarios](docs/scenarios.md) - Test scenarios
- [Solvers](docs/solvers.md) - Solver implementations
- [Next Steps](docs/next-steps.md) - Implementation roadmap

## Development

```bash
# Run all checks
just check-all

# Format code
just fmt

# Run lints
just lint

# Build documentation
just doc-open
```

## Key Concepts

### Frequent Batch Auction (FBA)

Orders are collected over a time window and matched simultaneously at a uniform clearing price. This prevents front-running and ensures fair execution.

### Welfare Maximization

The solver maximizes total welfare:
```
welfare = Σ (limit_price - clearing_price) × fill_qty
```
across all filled orders.

### Bundle Orders

Orders can span multiple markets with arbitrary payoff structures:
- Spreads: "Long A, Short B"
- Bundles: "A AND B must both win"
- Conditionals: "Buy if price > threshold"

## License

[TBD]
