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
cargo run -p matching-sim --release -- --markets 20 --orders 500 --bundles 0.2 --solver lp -v
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

**Solver Selection:** `--solver <lp|eg|conic|milp|all>`

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
| `lp` | Default. LP via HiGHS with entropy smoothing |
| `eg` | Eisenberg-Gale / Fisher market formulation |
| `conic` | Conic EG via Clarabel |
| `milp` | SCIP-based MIQCQP (exact with timeout) |
| `all` | Run all solvers and compare metrics |

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
matching-solver    →  PipelineResult (fills, welfare, prices)
        ↓
sybil-verifier     →  VerificationResult
        ↓
VizSnapshot        →  JSON for Streamlit
```
