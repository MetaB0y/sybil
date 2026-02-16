# Sybil

A prediction market matching engine using Frequent Batch Auctions (FBA) with cross-market order support.

## Features

- **Payoff vector orders** - Unified representation for simple orders, bundles, spreads, conditionals
- **Batch auction matching** - Uniform clearing price prevents front-running
- **Multi-phase solver pipeline** - Per-market clearing, cross-market optimization, MM budget allocation
- **Market maker constraints** - Budget-constrained quoting across markets
- **MILP benchmark** - Feature-gated optimal solver for comparison
- **ZK-ready verification** - Solver output validation designed for proof integration

## Quick Start

```bash
just test             # Run all tests
just sim-quick        # Run a small simulation (~50 orders)
just compare          # Compare all solvers on medium scenario
just check-all        # fmt + lint + test
```

## Project Structure

```
sybil/
├── crates/
│   ├── matching-engine/     # Core types: orders, fills, markets, payoff vectors
│   ├── matching-solver/     # Solver pipeline and algorithms
│   ├── matching-scenarios/  # Test scenario generators
│   ├── matching-sim/        # CLI simulation tool
│   ├── matching-sequencer/  # Multi-batch sequential simulation
│   ├── sybil-api/           # HTTP API server for agent trading
│   ├── sybil-oracle/        # Oracle/resolution service
│   └── sybil-verifier/      # Block verification service
├── arena/                   # Python: trading bots, client SDK, backtesting
├── viz/                     # Streamlit visualization dashboard
├── design/                  # Internal design notes and research
├── docs/                    # Public documentation (Mintlify)
└── justfile                 # Task runner (run `just` for all commands)
```

## Documentation

- [Architecture](design/architecture.md) - Pipeline design and solver phases
- [Solver Research](design/solver-research.md) - MILP gap analysis and improvement approaches
- [Welfare vs Volume](design/welfare-vs-volume.md) - Optimization objective tradeoffs
- [Public Docs](docs/) - Mintlify documentation site

## How It Works

Orders are collected over a time window and matched simultaneously at a uniform clearing price per market. The solver maximizes **total welfare** (consumer surplus):

```
welfare = Σ (limit_price - clearing_price) × fill_qty
```

The solver pipeline:

1. **LocalSolver** - Per-market price discovery, O(n log n)
2. **NegriskSolver** - Cross-market arbitrage exploitation
3. **MmAllocator** - Budget-constrained market maker allocation
4. **MILP** (optional) - Exact optimal solution with timeout

Default mode uses **dual decomposition** (Lagrangian relaxation) for joint price consistency and MM budget handling.

## License

[TBD]
