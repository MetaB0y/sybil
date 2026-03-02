# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-solver** crate is the core optimization engine for welfare-maximizing order matching in Frequent Batch Auctions (FBA). It solves an NP-hard problem via convex programs.

**Key metric**: `welfare = Σ (limit_price - clearing_price) × fill_qty`

## Solver Types

| Solver | File | Feature | Purpose |
|--------|------|---------|---------|
| **LpSolver** | `lp_solver.rs` | `lp` | LP via HiGHS with entropy smoothing. Best welfare across all presets. |
| **EgSolver** | `eg_solver.rs` | `lp` | Eisenberg-Gale / Fisher market formulation. |
| **ConicSolver** | `conic_solver.rs` | `conic` | Conic EG via Clarabel. |
| **IterLpSolver** | `iterative_lp_solver.rs` | `lp` | Iterative LP with EG μ-boosted MM weights. |
| **MilpSolver** | `milp.rs` | `milp` | MIQCQP via SCIP (russcip). Exact optimal with timeout. |
| **DecomposedSolver** | `decomposed.rs` | `lp` (+`parallel`) | Per-market-group decomposition with mirror descent budget coordination. Wraps any ComponentSolver. |

All solvers return a `PipelineResult` which contains `MatchingResult` (fills + welfare), clearing prices, and timing data.

## Key Files

| File | Purpose |
|------|---------|
| `lib.rs` | Crate root, `MatchingResult` type |
| `result.rs` | `PipelineResult`, `PriceDiscoveryResult`, timing/stats types |
| `lp_solver.rs` | LP-based solver via HiGHS |
| `eg_solver.rs` | Eisenberg-Gale solver |
| `conic_solver.rs` | Conic solver via Clarabel |
| `milp.rs` | SCIP-based MIQCQP solver |
| `verifier.rs` | Result verification for ZK integration |
| `decomposed.rs` | Per-market-group decomposition with rayon parallelism (`parallel` feature) |
| `viz.rs` | Visualization snapshots and ASCII output |

## Testing

```bash
cargo test -p matching-solver                           # Basic tests
cargo test -p matching-solver --features lp,conic,milp  # All solvers
```

## Design Principles

- **All integer arithmetic**: Prices/quantities in nanos (1 dollar = 1,000,000,000 nanos)
- **Payoff vectors**: Orders are payoff vectors over market states (single-market binary for now)
- **Welfare maximization**: Objective is `Σ (limit_price - clearing_price) * fill_qty`
- **Group minting**: LP/EG/Conic handle cross-market arbitrage via gmint variables
- **Verification**: `verifier.rs` validates solver output for correctness (ZK-ready)
