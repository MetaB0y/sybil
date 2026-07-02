# Sybil

A prediction market matching engine using Frequent Batch Auctions (FBA) with cross-market order support.

## Features

- **Payoff vector orders** - Unified representation for simple orders, bundles, spreads, conditionals
- **Batch auction matching** - Uniform clearing price prevents front-running
- **Multiple solvers** - LP (production), Eisenberg-Gale, Conic, MILP (exact), Decomposed (parallel)
- **Market maker constraints** - Budget-constrained quoting across markets
- **ZK-ready verification** - Solver output validation designed for proof integration
- **AI Arena** - Python SDK + LLM-powered trading bots for backtesting

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
│   ├── matching-solver/     # Solver implementations (LP, EG, Conic, MILP, Decomposed)
│   ├── matching-scenarios/  # Test scenario generators
│   ├── matching-sim/        # CLI simulation tool with presets and solver comparison
│   ├── matching-sequencer/  # Multi-batch sequential simulation with agents
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

- [System Specification](docs/SPEC.md) - **Start here.** One linear document covering the whole system: domain model, matching problem, solvers, sequencer, verification, ZK pipeline, contracts, API, arena, deployment, and invariants
- [Architecture vault](docs/architecture/Sybil%20Architecture.md) - Obsidian vault of ~48 per-concept canonical notes
- [Architecture Review 2026-07](design/architecture-review-2026-07.md) - Simplification proposals and known doc drift
- [Solver Benchmarks](design/solver-benchmarks.md) - Comparative evaluation of all solvers
- [Welfare vs Volume](design/welfare-vs-volume.md) - Optimization objective tradeoffs

## How It Works

Orders are collected over a time window and matched simultaneously at a uniform clearing price per market. The solver maximizes **total welfare** (consumer surplus):

```
welfare = Σ (limit_price - clearing_price) × fill_qty
```

### Solvers

All solvers take a `Problem` and return a `PipelineResult` (fills, clearing prices, welfare, timing).

| Solver | Backend | Description |
|--------|---------|-------------|
| **LpSolver** | HiGHS | LP + single-pass SLP MM budget shading. Production default. |
| **IterLpSolver** | HiGHS | Damped fixed-point on the Eisenberg-Gale budget multiplier; better under tight MM budgets. |
| **EgSolver** | HiGHS | Eisenberg-Gale / Fisher market formulation (Frank-Wolfe). |
| **ConicSolver** | Clarabel | Interior-point solver with configurable objective (Linear, Fisher, QuasiFisher). |
| **MilpSolver** | SCIP | Mixed-integer exact optimal with timeout. Feature-gated (`milp`). |
| **DecomposedSolver** | (wraps any) | Per-market-group decomposition with mirror descent budget coordination. |

The **LpSolver** is the production default — fastest (0.17s on medium) and highest welfare ($62.63K).

## License

[TBD]
