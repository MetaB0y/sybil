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
│   ├── matching-solver/           # Solver implementations (LP, EG, Conic, MILP, Decomposed)
│   ├── matching-scenarios/        # Test scenario generators (order mixes, spreads)
│   ├── matching-sim/              # CLI simulation tool with presets and solver comparison
│   ├── matching-sequencer/        # Agent-based multi-batch sequential simulation
│   ├── sybil-api/                 # HTTP API server for agent trading
│   ├── sybil-oracle/              # Oracle/resolution service
│   └── sybil-verifier/            # ZK-ready block verification
├── arena/                         # Python: trading bots, client SDK, simulation framework (has its own CLAUDE.md)
│   ├── sim/                       #   Generic simulation framework (clock, news_trader, runner)
│   ├── markets/                   #   Per-market config (iran/ with personas, sources, datasets)
│   └── viz/                       #   Streamlit dashboards
├── viz/                           # Python: Streamlit visualization dashboard (Rust solver)
├── fuzz/                          # Cargo-fuzz targets (separate workspace)
├── design/                        # Historical design notes (superseded by docs/architecture/)
│   ├── architecture.md            #   [superseded] Solver design, key abstractions
│   ├── architecture-diagrams.md   #   [superseded] System overview diagrams
│   ├── solver-benchmarks.md       #   Comparative solver evaluation
│   └── welfare-vs-volume.md       #   Optimization objective tradeoffs (deep-dive)
├── docs/architecture/             # Obsidian vault — canonical architecture spec (~35 notes)
├── scripts/check-vault.sh         # Vault validation (links, frontmatter, staleness)
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
| **LpSolver** | `lp_solver.rs` | `lp` | LP via HiGHS + entropy smoothing + iterative MM budget shading. **Production default.** |
| **EgSolver** | `eg_solver.rs` | `lp` | Eisenberg-Gale / Fisher market formulation. |
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

An Obsidian vault at `docs/architecture/` is the canonical architectural spec. ~35 interlinked notes covering every major concept. Notes use `[[wiki-links]]` and YAML frontmatter (`tags`, `layer`, `status`, `last_verified`).

**Entry point**: `docs/architecture/Sybil Architecture.md`

**When to read**: Before modifying any crate, read the notes listed in that crate's CLAUDE.md under "Architecture Notes". This gives you the design context and invariants you need to preserve.

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

**Server**: Linode g6-standard-1 (2GB), Debian 13, IP in `config/deploy.yml`.

**Deploy tool**: Kamal 2. Builds Docker image locally, pushes to GHCR, deploys via SSH.

**IMPORTANT**: Claude Code cannot deploy. Docker buildx requires host networking which is unavailable in the sandboxed environment. When deploying:
1. Remind the user to run commands from their own terminal
2. Provide the exact commands
3. Never attempt `kamal deploy`, `docker build`, or remote builds yourself

### Deploy commands (run from your terminal, not Claude Code)

```bash
# First time setup (installs Docker on server, starts proxy)
kamal setup

# Deploy latest code
kamal deploy

# Boot polymarket mirror accessory
kamal accessory boot polymarket

# View logs
kamal app logs -f                   # sybil-api
kamal accessory logs polymarket     # polymarket mirror

# Restart after config change
kamal app restart
kamal accessory restart polymarket
```

### Manual deploy (if Kamal has issues)

```bash
# Build locally
docker build -t ghcr.io/metab0y/sybil-api:latest .
docker push ghcr.io/metab0y/sybil-api:latest

# On server
ssh root@<server-ip>
docker pull ghcr.io/metab0y/sybil-api:latest
docker stop sybil-api sybil-polymarket
docker rm sybil-api sybil-polymarket
docker run -d --name sybil-api --restart unless-stopped \
  -p 3000:3000 \
  -e SYBIL_DEV_MODE=true -e SYBIL_BLOCK_INTERVAL_MS=2000 -e RUST_LOG=info \
  ghcr.io/metab0y/sybil-api:latest
docker run -d --name sybil-polymarket --restart unless-stopped \
  -v polymarket-data:/data -e RUST_LOG=sybil_polymarket=info \
  --entrypoint sybil-polymarket \
  ghcr.io/metab0y/sybil-api:latest \
  --sybil-url http://172.17.0.1:3000 --max-events 50 --mm-half-spread 0.02 \
  --mm-budget-dollars 5000 --mm-initial-balance-dollars 1000000 \
  --mapping-store-path /data/polymarket_mapping.json
```

### Dashboard

`http://<server-ip>:3000/` — Alpine.js single-page view of markets, MM state, live blocks.

## Development Notes

- Do not use floating point numbers, use u64 etc.
- Use proptest for property-based/metamorphic tests but only where it makes sense
- Always think about boundaries and reducing accidental complexity -- avoid tight coupling unless necessary
- Prefer actor model using this pattern https://ryhl.io/blog/actors-with-tokio/ to mutex etc.
- We are in early dev phase. Elegance is always more important than backward compatibility
