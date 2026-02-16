# Sybil V2

A prediction market matching engine using Frequent Batch Auctions (FBA) with cross-market order support.

## Features

- **Linear constraint orders** - Orders expressed as payoff vectors, supporting complex multi-market strategies
- **Batch auction matching** - Uniform clearing price prevents front-running
- **Two-phase solving** - Per-market clearing, then cross-market optimization
- **MM budget constraints** - Market makers can quote across markets with budget constraints
- **Cross-market orders** - Bundles, spreads, conditionals across correlated markets
- **Solution combination** - MWIS-based combination of solver outputs

## Quick Start

```bash
# Run tests
just test

# Run benchmarks
cargo bench -p matching-solver

# Run validation tests
cargo test -p matching-solver --test validation
```

## Project Structure

```
sybil/
├── crates/
│   ├── matching-engine/     # Core types, orders, fills, markets, payoff vectors
│   ├── matching-solver/     # Solver pipeline and algorithms
│   ├── matching-scenarios/  # Test scenario generators
│   ├── matching-sim/        # CLI simulation tool
│   ├── matching-sequencer/  # Multi-batch sequential simulation
│   ├── sybil-api/           # API server
│   ├── sybil-oracle/        # Oracle service
│   └── sybil-verifier/      # Verification service
├── design/                  # Internal design notes and research
├── docs/                    # Public documentation (Mintlify)
├── arena/                   # Python client, backtesting, bots
├── viz/                     # Streamlit visualization dashboard
└── justfile                 # Build/test commands
```

## Documentation

- [Architecture](design/architecture.md) - Pipeline design and solver phases
- [Solver Research](design/solver-research.md) - MILP gap analysis and improvement approaches
- [Welfare vs Volume](design/welfare-vs-volume.md) - Optimization objective tradeoffs
- [Public Docs](docs/) - Mintlify documentation site

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

### Two-Phase Architecture

```
Phase 1: Per-Market Clearing
  - Find clearing prices where Σp_i = 1
  - Fast, O(n log n), parallelizable

Phase 2: Cross-Market Optimization
  - MM budget allocation via Lagrangian relaxation
  - Bundle/spread orders via specialized solvers
  - Combine via MWIS on conflict graph
```

### Bundle Orders

Orders can span multiple markets with arbitrary payoff structures:
- Spreads: "Long A, Short B"
- Bundles: "A AND B must both win"
- Conditionals: "Buy if price > threshold"

## License

[TBD]
