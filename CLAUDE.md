# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Version Control

This project uses **jj (Jujutsu)** for version control, NOT git.

- `jj status` instead of `git status`
- `jj log` instead of `git log`
- `jj diff --git` instead of `git diff`
- `jj new` to create new changes
- `jj describe` to set commit messages

## Repo Map

```
sybil/
├── crates/                        # Rust workspace
│   ├── matching-engine/           # Core types: orders, fills, markets, payoff vectors, MM constraints
│   ├── matching-solver/           # Solver pipeline and algorithms (~7k lines, most dev happens here)
│   ├── matching-scenarios/        # Test scenario generators (order mixes, bundles, spreads)
│   ├── matching-sim/              # CLI simulation tool with presets and solver comparison
│   ├── matching-sequencer/        # Agent-based multi-batch sequential simulation
│   ├── sybil-api/                 # HTTP API server for agent trading
│   ├── sybil-oracle/              # Oracle/resolution service
│   └── sybil-verifier/            # ZK-ready block verification
├── arena/                         # Python: trading bots, client SDK, backtesting (has its own CLAUDE.md)
├── viz/                           # Python: Streamlit visualization dashboard
├── fuzz/                          # Cargo-fuzz targets (separate workspace)
├── design/                        # Internal design notes
│   ├── architecture.md            #   Pipeline design, solver phases, integration points
│   ├── solver-research.md         #   MILP gap analysis, 3 improvement approaches
│   └── welfare-vs-volume.md       #   Optimization objective tradeoffs
├── docs/                          # Public documentation (Mintlify site)
├── CLAUDE.md                      # This file
├── justfile                       # Task runner (run `just` to see all commands)
└── Cargo.toml                     # Workspace root
```

Each crate has its own CLAUDE.md with detailed architecture notes.

## Build & Development Commands

```bash
just build            # cargo build --release
just test             # cargo test --workspace
just lint             # cargo clippy --workspace --all-features
just fmt              # cargo fmt --all
just check-all        # fmt-check + lint + test (CI equivalent)
just bench            # cargo bench --workspace
just doc              # cargo doc --workspace --no-deps
```

Run a single test:
```bash
cargo test -p matching-solver test_name
```

### Simulation

```bash
just sim-quick        # ~50 orders
just sim-small        # ~300 orders
just sim-medium       # ~3000 orders
just compare          # Compare all solvers on medium scenario
just sim preset solver # Custom: just sim large negrisk
just milp-killer      # MILP stress test (forces timeout)
```

### MILP Solver (feature-gated)

```bash
cargo run --release -p matching-sim --features milp -- --preset quick --solver all
cargo run --release -p matching-sim --features milp -- --preset small --solver milp --milp-timeout 60 --mm-mode exact
```

### Arena (Python bots)

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001  # Start server
cd arena && uv sync && uv run python examples/full_competition.py
just arena-demo       # All-in-one: start server + run backtest
```

### Visualization

```bash
just viz-run          # Generate snapshot + launch Streamlit dashboard
```

### Fuzzing

```bash
cd fuzz && cargo fuzz run fuzz_order_parse
cd fuzz && cargo fuzz run fuzz_settlement
```

## Architecture

Sybil is a **prediction market matching engine** built on Frequent Batch Auctions (FBA). It solves the welfare-maximizing order matching problem (NP-hard in general) via a multi-phase pipeline.

### Solver Pipeline (matching-solver)

The pipeline runs in phases, orchestrated by `pipeline.rs`:

1. **LocalSolver** (`local_solver.rs`): Per-market price discovery. O(n log n), handles ~80% of single-market orders. Finds clearing prices where outcome prices sum to $1.
2. **NegriskSolver** (`specialized/negrisk.rs`): Exploits price inconsistencies across related markets (arbitrage). Creates synthetic fills when prices don't perfectly sum to $1.
3. **MmAllocator** (`mm_allocator.rs`): Allocates market maker fills respecting budget constraints. Greedy allocation by welfare/capital ratio with fixed-point iteration.
4. **Partial Solvers** (parallel): `MilpSolver` (ILP, optimal with timeout; feature-gated behind `milp`).

The default pipeline is `Pipeline::with_dual_decomposition()` which uses `DualMaster` for Lagrangian relaxation of price consistency + MM budgets.

**Other solvers** (exported but not in default pipeline):
- `lp_solver.rs`: LP via HiGHS + iterative MM budget shading. Feature-gated: `lp`. Best welfare across all presets.
- `joint_solver.rs`: Joint group optimization via parametric search on Σp=$1 simplex (volume-oriented)

### Key Design Decisions

- **Payoff vectors**: Orders are represented as payoff vectors over market states, enabling unified handling of simple orders, bundles, spreads, and conditionals.
- **Welfare maximization**: The objective is `Σ (limit_price - clearing_price) * fill_qty`, not volume.
- **MILP is optional**: Feature-gated behind `milp` (uses SCIP via `russcip`). Supports group-level minting for optimal negrisk-style arbitrage.
- **Verification** (`verifier.rs`): Validates solver output for correctness — designed for ZK proof integration.
- **All integer arithmetic**: No floating point. Prices/quantities in nanos (1 dollar = 1,000,000,000 nanos).

## Development Notes

- Do not use floating point numbers, use u64 etc.
- Use proptest for property-based/metamorphic tests but only where it makes sense
- Always think about boundaries and reducing accidental complexity -- avoid tight coupling unless necessary
- Prefer actor model using this pattern https://ryhl.io/blog/actors-with-tokio/ to mutex etc.
- We are in early dev phase. Elegance is always more important than backward compatibility
