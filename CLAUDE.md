# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Version Control

This project uses **jj (Jujutsu)** for version control, NOT git.

- `jj status` instead of `git status`
- `jj log` instead of `git log`
- `jj diff --git` instead of `git diff`
- `jj new` to create new changes
- `jj describe` to set commit messages

## Build & Development Commands

The project uses `just` as a task runner. Run `just` to see all available commands.

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

Run validation tests specifically:
```bash
cargo test -p matching-solver --test validation
```

### Simulation

```bash
just sim-quick        # ~50 orders
just sim-small        # ~300 orders
just sim-medium       # ~3000 orders
just compare          # Compare all solvers on medium scenario
just sim preset solver # Custom: just sim large negrisk
```

### Visualization (Python/Streamlit)

```bash
just viz-install      # Install Python deps via uv
just viz-run          # Generate snapshot + launch dashboard
```

## Architecture

Sybil is a **prediction market matching engine** built on Frequent Batch Auctions (FBA). It solves the welfare-maximizing order matching problem (NP-hard in general) via a multi-phase pipeline.

### Workspace Crates

- **matching-engine**: Core types — orders, fills, markets, order books, payoff vectors, MM constraints. All markets are binary (YES/NO); multi-outcome events are modeled as groups of binary markets.
- **matching-solver**: The solver pipeline and all matching algorithms (~7k lines). This is where most development happens.
- **matching-scenarios**: Test scenario generators with configurable order mixes (bundles, spreads, AON, MM orders).
- **matching-sim**: CLI tool for running simulations with presets and solver comparison.
- **matching-sequencer**: Agent-based multi-batch sequential simulation.

### Solver Pipeline (matching-solver)

The pipeline runs in phases, orchestrated by `pipeline.rs`:

1. **LocalSolver** (`local_solver.rs`): Per-market price discovery. O(n log n), handles ~80% of single-market orders. Finds clearing prices where outcome prices sum to $1.
2. **NegriskSolver** (`specialized/negrisk.rs`): Exploits price inconsistencies across related markets (arbitrage). Creates synthetic fills when prices don't perfectly sum to $1.
3. **MmAllocator** (`mm_allocator.rs`): Allocates market maker fills respecting budget constraints via Lagrangian relaxation. Greedy allocation by welfare/capital ratio with fixed-point iteration for interacting MMs.
4. **Partial Solvers** (run in parallel): `MilpSolver` (ILP, optimal with timeout; feature-gated behind `milp`).

The pipeline can iterate via fixed-point loop until convergence.

### Key Design Decisions

- **Payoff vectors**: Orders are represented as payoff vectors over market states, enabling unified handling of simple orders, bundles, spreads, and conditionals.
- **Welfare maximization**: The objective is `sum((limit_price - clearing_price) * fill_qty)`, not volume.
- **MILP is optional**: The `milp` feature (using HiGHS solver via `good_lp`) is feature-gated since it adds a large dependency.
- **Verification** (`verifier.rs`): Validates solver output for correctness — designed for ZK proof integration.

## Developemnt notes
- Do not use floating point numbers, use u64 etc.
- Use proptest for property-based/metamorphic tesets but only where it makes sense
- Always think about boundaries and reducing accidental complexity -- avoid tight coupling unless necessary
- Prefer actor model using this pattern https://ryhl.io/blog/actors-with-tokio/ to mutex etc.
