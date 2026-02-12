# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-sim** crate is a CLI simulation harness for testing and benchmarking the matching engine. It compares solver implementations, validates correctness, and generates visualization data.

## Usage

```bash
# Basic presets
cargo run -p matching-sim --release -- --preset quick -v
cargo run -p matching-sim --release -- --preset medium --solver all

# With MILP (feature-gated)
cargo run -p matching-sim --release --features milp -- --preset small --solver milp --milp-timeout 60

# Custom scenario
cargo run -p matching-sim --release -- --markets 20 --orders 500 --bundles 0.2 --solver pipeline -v
```

## CLI Options

**Presets:** `--preset <quick|small|medium|large|extreme|milp-killer>`

**Custom Scenario:**
- `--markets N` — number of binary markets
- `--orders N` — total orders
- `--bundles F` — bundle fraction (0.0-1.0)
- `--spreads F` — spread fraction
- `--scarcity F` — liquidity scarcity (lower = scarcer)
- `--mms N` — number of MM constraints
- `--seed N` — random seed

**Solver Selection:** `--solver <pipeline|negrisk|dual|milp|all>`

**MILP Options:**
- `--milp-timeout S` — time limit in seconds (default: 5.0)
- `--mm-mode <exact|mccormick|ignore>` — MM budget constraint handling

**Output:**
- `-v, --verbose` — detailed step-by-step output
- `--export-json PATH` — save VizSnapshot for Streamlit dashboard
- `--show-charts` — ASCII convergence charts
- `--batches N` — run N independent batches

## Solvers

| Solver | Description |
|--------|-------------|
| `pipeline` | Default. LocalSolver → NegriskSolver → MmAllocator (fixed-point) |
| `negrisk` | Pipeline with negrisk arbitrage emphasis |
| `dual` | Dual decomposition with Lagrangian relaxation |
| `milp` | SCIP-based MIQCQP (exact with timeout) |
| `all` | Run all solvers and compare metrics |

## Output Metrics

The results table shows per-solver:
- **Welfare** — total value captured (in $)
- **Fill %** — percentage of orders filled
- **Volume** — total quantity matched
- **Time** — execution time

Verbose mode (`-v`) adds:
- Problem summary (order composition, MM constraints)
- Per-iteration convergence table
- Fill statistics by order type
- Sample market prices
- Verification result

## Verification

All solver outputs are validated via `sybil-verifier`:
- Order constraints (quantity, price limits)
- MM budget constraints
- Welfare computation
- No duplicate fills

## Integration

```
matching-scenarios  →  Problem
        ↓
matching-solver    →  MatchingResult (fills, welfare)
        ↓
sybil-verifier     →  VerificationResult
        ↓
VizSnapshot        →  JSON for Streamlit
```
