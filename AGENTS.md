# AGENTS.md

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
│   ├── matching-solver/           # Solver implementations (LP, EG, Conic, MILP, Decomposed)
│   ├── matching-scenarios/        # Test scenario generators (order mixes, spreads)
│   ├── matching-sim/              # CLI simulation tool with presets and solver comparison
│   ├── matching-sequencer/        # Multi-batch block sequencer (production lib; linked by sybil-api)
│   ├── sequencer-sim/             # Dev-only agent-based simulation driving the sequencer (bin: sybil-sim)
│   ├── sybil-api/                 # HTTP API server for agent trading
│   ├── sybil-oracle/              # Oracle/resolution service
│   └── sybil-verifier/            # ZK-ready block verification
├── arena/                         # Python: trading bots, client SDK, simulation framework (has its own AGENTS.md)
│   ├── sim/                       #   Generic simulation framework (clock, news_trader, runner)
│   ├── markets/                   #   Per-market config (iran/ with personas, sources, datasets)
│   └── viz/                       #   Streamlit dashboards
├── contracts/                     # Solidity + Foundry L1 settlement/vault contracts
├── viz/                           # Python: Streamlit visualization dashboard (Rust solver)
├── fuzz/                          # Cargo-fuzz targets (separate workspace)
├── design/                        # Historical design notes (superseded by docs/architecture/)
│   ├── architecture.md            #   [superseded] Solver design, key abstractions
│   ├── architecture-diagrams.md   #   [superseded] System overview diagrams
│   ├── solver-benchmarks.md       #   Comparative solver evaluation
│   └── welfare-vs-volume.md       #   Optimization objective tradeoffs (deep-dive)
├── docs/architecture/             # Obsidian vault — canonical architecture spec (~35 notes)
├── scripts/check-vault.sh         # Vault validation (links, frontmatter, staleness)
├── AGENTS.md                      # This file
├── justfile                       # Task runner (run `just` to see all commands)
└── Cargo.toml                     # Workspace root
```

Each crate has its own AGENTS.md with detailed architecture notes.

## Build & Development Commands

```bash
just build            # cargo build --release
just test             # cargo test --workspace
just lint             # cargo clippy --workspace --all-features
just fmt              # cargo fmt --all
just check-all        # fmt-check + lint + test (CI equivalent)
just contracts-test   # forge test in contracts/
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
just sim preset solver # Custom: just sim large lp
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

Sybil is a **prediction market matching engine** built on Frequent Batch Auctions (FBA). It solves the welfare-maximizing clearing problem via convex programs.

### Solvers (matching-solver)

All solvers take a `Problem` and return a `PipelineResult` (fills, clearing prices, welfare, timing). Feature-gated.

| Solver | File | Feature | Description |
|--------|------|---------|-------------|
| **LpSolver** | `lp_solver.rs` | `lp` | LP via HiGHS + single-pass SLP MM budget shading. **Production default.** |
| **IterLpSolver** | `iterative_lp_solver.rs` | `lp` | Damped fixed-point on the EG budget multiplier; better under tight MM budgets. |
| **EgSolver** | `eg_solver.rs` | `lp` | Eisenberg-Gale / Fisher market formulation (Frank-Wolfe). |
| **ConicSolver** | `conic_solver.rs` | `conic` | Interior-point via Clarabel. Configurable objective (Linear, Fisher, QuasiFisher). |
| **MilpSolver** | `milp.rs` | `milp` | SCIP MIQCQP. Exact optimal with timeout. |
| **DecomposedSolver** | `decomposed.rs` | `lp` | Per-market-group decomposition with mirror descent budget coordination. |

### Key Design Decisions

- **Payoff vectors**: Orders are represented as payoff vectors over market states, enabling unified handling of simple orders, spreads, and conditionals.
- **Welfare maximization**: The objective is `Σ (limit_price - clearing_price) * fill_qty`, not volume.
- **Fisher market structure**: MM budgets can be absorbed into the EG objective (no explicit budget constraints). See `lmsr-proof.typ`.
- **Verification** (`verifier.rs`): Validates solver output for correctness — designed for ZK proof integration.
- **All integer arithmetic**: No floating point. Prices/quantities in nanos (1 dollar = 1,000,000,000 nanos).

## Architecture Knowledge Base

An Obsidian vault at `docs/architecture/` is the canonical architectural spec. ~48 interlinked notes covering every major concept. Notes use `[[wiki-links]]` and YAML frontmatter (`tags`, `layer`, `status`, `last_verified`).

**Entry point**: `docs/architecture/Sybil Architecture.md`

**Linear walkthrough**: `docs/SPEC.md` is a single connected document covering the whole system (domain model → solvers → sequencer → verification → ZK → contracts → API → arena → ops → invariants). Read it for orientation; use the vault for per-concept depth. `design/architecture-review-2026-07.md` tracks simplification proposals and known doc drift.

**When to read**: Before modifying any crate, read the notes listed in that crate's AGENTS.md under "Architecture Notes". This gives you the design context and invariants you need to preserve.

**When to update**: After significant architectural changes (new solver, new crate, changed data flow), update the relevant note(s) and run `just docs-check` to validate.

### Quick Reference

| Topic | Notes |
|-------|-------|
| Core model | Payoff Vectors, Binary Markets and Market Groups, Nanos and Integer Arithmetic, Order Types |
| Solvers | Solver Landscape, LP Solver, EG Solver, Conic Solver, MILP Solver, Decomposed Solver, The LP Core |
| Sequencer | Block Lifecycle, Mempool, Settlement, Pending Orders and TTL |
| API | REST API, SSE Block Stream, P256 Authentication |
| Oracle | Oracle Lifecycle, Market Resolution |
| Verification | Four-Layer Verification, Block Witness, ZK Integration Path |
| Economics | Welfare Maximization, Welfare vs Volume, MM Budget Constraint, LP Duality and Clearing Prices, Minting |
| Arena | Bot Framework, LLM Trader, Python SDK |

### Vault Commands

```bash
just docs-check               # Validate links, frontmatter, staleness
just docs-list                 # List all notes with layer + status
just docs-stale                # List notes with last_verified > 90 days
just docs-search "welfare"     # Grep vault content
just docs-rename old new       # Rename note + update wiki-links (needs notesmd-cli)
just docs-read "LP Solver"     # Print note with linked mentions (needs notesmd-cli)
just docs-verify "LP Solver"   # Set last_verified to today (needs notesmd-cli)
```

## Deployment

**Server**: Linode g6-standard-1 (2GB), Debian 13, IP `172.104.31.54`.

**Deploy method**: Build locally, `docker save | ssh docker load`, restart containers. All via justfile recipes.

```bash
just deploy-api                    # Build + deploy sybil-api + polymarket mirror
just deploy-arena $OPENROUTER_KEY  # Build + deploy arena bots
just deploy-dashboard              # Deploy Streamlit dashboard
just deploy-all $OPENROUTER_KEY    # Everything at once

just deploy-logs                   # Tail sybil-api logs
just deploy-logs sybil-polymarket  # Tail polymarket mirror logs
just deploy-logs sybil-arena       # Tail arena bot logs
just deploy-shell                  # SSH into server
```

### Dashboards

- `http://172.104.31.54:3000/` — Alpine.js: markets, MM state, live blocks
- `http://172.104.31.54:8501/` — Streamlit: arena bot decisions, PnL, news feed

## Development Notes

- Do not use floating point numbers, use u64 etc.
- Use proptest for property-based/metamorphic tests but only where it makes sense
- Always think about boundaries and reducing accidental complexity -- avoid tight coupling unless necessary
- Prefer actor model using this pattern https://ryhl.io/blog/actors-with-tokio/ to mutex etc.
- We are in early dev phase. Elegance is always more important than backward compatibility
