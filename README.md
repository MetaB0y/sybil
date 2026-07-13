# Sybil

A validity-oriented prediction-market exchange built around frequent batch
auctions, deterministic integer settlement, and agent-first interfaces.

## Features

- **Payoff vector model** - Unified representation; current production clearing intentionally accepts a narrower single-market subset
- **Batch auction matching** - Eligible orders clear together at a uniform price
- **Multiple solvers** - LP (production), Eisenberg-Gale, Conic, MILP (exact), Decomposed (parallel)
- **Market maker constraints** - Budget-constrained quoting across markets
- **Validity pipeline** - Native/OpenVM transition verification, proof artifacts, DA, and L1 settlement components
- **AI Arena** - Python SDK, simulations, and live LLM-powered trading agents

## Quick Start

```bash
just test             # Run all tests
just sim-quick        # Run a small simulation (~50 orders)
just compare          # Compare all solvers on medium scenario
just check-fast       # Fast default-feature Rust check + clippy
just check-features   # Exhaustive all-target/all-feature Rust check + clippy
just check-consensus  # protocol and guest commitment consistency
just check-all        # complete CI-equivalent gate
```

## Project Structure

```
sybil/
├── crates/
│   ├── matching-*           # Domain model, solvers, sequencer, scenarios, and simulations
│   ├── sybil-api*           # HTTP/realtime service and shared wire types
│   ├── sybil-signing/       # Canonical client signing bytes
│   ├── sybil-verifier/      # Native canonical transition verification
│   ├── sybil-zk/            # Guest-safe verification and public inputs
│   ├── sybil-prover/        # Proof jobs, artifacts, DA, and L1 submission
│   ├── sybil-l1-*/          # Bridge protocol types and L1 indexer
│   ├── sybil-oracle/        # Signed market-resolution policy
│   ├── sybil-polymarket/    # External mirror and reference liquidity
│   └── sybil-client/        # Shared Rust client
├── contracts/               # Solidity vault, settlement, and verifier adapters
├── arena/                   # Python agents, SDK, simulation, and dashboards
├── frontend/web/            # Next.js trader and arena UI
├── design/                  # Proposals, proofs, research, and dated archives
├── docs/                    # Current system, decision, and operations docs
└── justfile                 # Task runner (run `just` for all commands)
```

## Documentation

- [Documentation guide](docs/README.md) — **start here** for the high-level model and reading paths
- [System specification](docs/SPEC.md) — one connected, implementation-oriented system description
- [Architecture map](docs/architecture/Sybil%20Architecture.md) — focused notes, diagrams, code pointers, and trust boundaries
- [Decision records](docs/adr/README.md) — why the load-bearing choices were made
- [Deployment runbook](DEPLOY.md) — the authoritative production deployment procedure
- [Design workspace](design/README.md) — proposals and research; not a description of shipped behavior

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

The **LpSolver** is the production default. Benchmark results are workload- and
revision-dependent; see [the benchmark notes](design/solver-benchmarks.md) and
rerun the repository benchmarks before quoting numbers.

## License

No license file is currently published. Treat the repository as all rights
reserved until the maintainers add one.
