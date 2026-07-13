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
│   ├── sybil-api-types/           # Shared REST/WebSocket DTOs and OpenAPI schema
│   ├── sybil-history/             # Private durable history projector and query service
│   ├── sybil-history-types/       # Neutral sequencer-to-history facts and query DTOs
│   ├── sybil-loadtest/            # Goose HTTP load and architectural-isolation checks
│   ├── sybil-client/              # Shared Rust HTTP/SSE client
│   ├── sybil-signing/             # Canonical client-action signing bytes
│   ├── sybil-oracle/              # Oracle/resolution service
│   ├── sybil-verifier/            # Native canonical witness/state verification
│   ├── sybil-zk/                  # Guest-safe transition/public-input verification
│   ├── sybil-prover/              # Proof jobs, DA artifacts, calldata/submission
│   ├── sybil-escape-claim/         # Guest-safe conservative escape statement
│   ├── sybil-custody/              # User snapshots, reconstruction, escape proving CLI
│   ├── sybil-l1-protocol/          # Guest-safe Rust/Solidity bridge hash domains
│   ├── sybil-l1-abi/               # Host-only Alloy contract bindings
│   ├── sybil-l1-indexer/           # L1 deposit/withdrawal lifecycle sidecar
│   └── sybil-polymarket/           # External market mirror and MM integration
├── arena/                         # Python: trading bots, client SDK, simulation framework (has its own AGENTS.md)
│   ├── sim/                       #   Generic simulation framework (clock, news_trader, runner)
│   ├── markets/                   #   Per-market config (iran/ with personas, sources, datasets)
│   └── viz/                       #   Streamlit dashboards
├── contracts/                     # Solidity + Foundry L1 settlement/vault contracts
├── viz/                           # Python: Streamlit visualization dashboard (Rust solver)
├── fuzz/                          # Cargo-fuzz targets (separate workspace)
├── design/                        # Proposals, proofs, research, and historical archive
├── docs/                          # Current spec, ADRs, runbooks, security/custody guides
│   └── architecture/              # Canonical architecture vault, grouped by system flow
├── scripts/check-vault.sh         # Vault validation (links, frontmatter, staleness)
├── AGENTS.md                      # This file
├── justfile                       # Task runner (run `just` to see all commands)
└── Cargo.toml                     # Workspace root
```

Each crate has its own AGENTS.md with focused ownership, boundaries, and checks.

## Build & Development Commands

```bash
just build            # cargo build --release
just test             # cargo test --workspace
just lint             # cargo clippy --workspace --all-features
just fmt              # cargo fmt --all
just check-fast       # Rust metadata + fmt + check + clippy
just check-consensus  # goldens + guest fingerprints + deployment pins
just check-all        # all workspaces and language/tooling gates (CI equivalent)
just clean            # cargo clean in root, fuzz, and all OpenVM workspaces
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
| **RetainedCashSolver** | `retained_cash_solver.rs` | `lp` | Certified generalized Frank--Wolfe on the affine-to-log retained-cash objective. **Production default.** |
| **LpSolver** | `lp_solver.rs` | `lp` | LP via HiGHS + single-pass SLP MM budget shading; low-latency baseline. |
| **IterLpSolver** | `iterative_lp_solver.rs` | `lp` | Explicit compatibility alias to RetainedCashSolver. |
| **EgSolver** | `eg_solver.rs` | `lp` | Explicit compatibility alias to RetainedCashSolver; no-cash ablation lives in Conic Fisher mode. |
| **ConicSolver** | `conic_solver.rs` | `conic` | Independent exponential-cone retained-cash reference via Clarabel. |
| **MilpSolver** | `milp.rs` | `milp` | SCIP MIQCQP. Exact optimal with timeout. |
| **DecomposedSolver** | `decomposed.rs` | `lp` | Per-market-group decomposition with mirror descent budget coordination. |

### Key Design Decisions

- **Payoff vectors**: Orders are represented as payoff vectors over market states, enabling unified handling of simple orders, spreads, and conditionals.
- **Welfare maximization**: The objective is `Σ (limit_price - clearing_price) * fill_qty`, not volume.
- **Fisher market structure**: MM budgets can be absorbed into the EG objective (no explicit budget constraints). See `paper.typ` in `~/github/prediction-markets-are-fisher-markets/` (pointer: `design/math-papers.md`).
- **Verification** (`verifier.rs`): Validates solver output for correctness — designed for ZK proof integration.
- **Integer protocol truth**: Prices, quantities, settlement, commitments, and verification use integers. Optimization libraries may search in floating point; only landed integer outputs are trusted.

## Architecture Knowledge Base

An Obsidian vault at `docs/architecture/` is the canonical architectural spec. Its interlinked notes cover the major concepts. Notes use `[[wiki-links]]` and YAML frontmatter (`tags`, `layer`, `status`, `last_verified`).

**Entry point**: `docs/architecture/Sybil Architecture.md`

**Linear walkthrough**: `docs/SPEC.md` is a single connected document covering the whole system (domain model → solvers → sequencer → verification → ZK → contracts → API → arena → ops → invariants). Read it for orientation; use the vault for per-concept depth. `design/` is proposal/research material and `design/archive/` is historical, not current-state guidance.

**When to read**: Before modifying any crate, read the notes listed in that crate's AGENTS.md under "Architecture Notes". This gives you the design context and invariants you need to preserve.

**When to update**: After significant architectural changes (new solver, new crate, changed data flow), update the relevant note(s) and run `just docs-check` to validate.

### Quick Reference

| Topic | Notes |
|-------|-------|
| Core model | Payoff Vectors, Binary Markets and Market Groups, Nanos and Integer Arithmetic, Order Types |
| Solvers | Solver Landscape, Retained Cash Solver, LP Solver, EG Solver, Conic Solver, MILP Solver, Decomposed Solver, The LP Core |
| Sequencer | Block Lifecycle, Order Admission, Settlement, Pending Orders and TTL |
| API | REST API, SSE Block Stream, P256 Authentication |
| Oracle | Market Resolution |
| Verification | Four-Layer Verification, Block Witness, ZK Integration Path |
| Economics | Welfare Maximization, Welfare vs Volume, MM Budget Constraint, LP Duality and Clearing Prices, Minting |
| Arena | Bot Framework, LLM Trader, Python SDK |

### Vault Commands

```bash
just docs-check               # Validate generated pins, inventories, vault, and site build
just docs-mermaid             # Render every maintained Mermaid diagram
just docs-links               # Check public and repository links
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
just deploy-arena                  # Build + deploy arena bots/dashboard
just deploy-web                    # Build + deploy Next.js frontend
just deploy-all                    # Build locally and deploy the complete API/arena/web/ops stack

just deploy-logs                   # Tail sybil-api logs
just deploy-logs sybil-polymarket  # Tail polymarket mirror logs
just deploy-logs sybil-arena       # Tail arena bot logs
just deploy-shell                  # SSH into server
```

### Dashboards

- `https://172-104-31-54.nip.io/` — public web UI and API
- `https://arena.172-104-31-54.nip.io/` — authenticated Streamlit arena dashboard
- `https://grafana.172-104-31-54.nip.io/` — authenticated operations dashboard

## Development Notes

- Do not introduce floating point into protocol state, settlement, commitments, or verification. Solver-internal search may use it behind the integer landing/verifier boundary.
- Use proptest for property-based/metamorphic tests but only where it makes sense
- Always think about boundaries and reducing accidental complexity -- avoid tight coupling unless necessary
- Prefer actor model using this pattern https://ryhl.io/blog/actors-with-tokio/ to mutex etc.
- We are in early dev phase. Elegance is always more important than backward compatibility
