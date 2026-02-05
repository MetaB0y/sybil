# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-solver** crate is the core optimization engine for welfare-maximizing order matching in Frequent Batch Auctions (FBA). It solves an NP-hard problem through a multi-phase pipeline.

**Key metric**: `welfare = Σ (limit_price - clearing_price) × fill_qty`

## Solver Types

| Solver | File | Purpose |
|--------|------|---------|
| **LocalSolver** | `local_solver.rs` | Per-market clearing, O(n log n). Guarantees P_YES + P_NO = $1 by construction. |
| **NegriskSolver** | `specialized/negrisk.rs` | Exploits arbitrage when group prices sum < $1. Creates synthetic orders. |
| **MmAllocator** | `mm_allocator.rs` | Greedy knapsack for MM budget constraints, welfare/capital ratio sorting. |
| **DualMaster** | `dual_master.rs` | Lagrangian relaxation for coupled constraints (price consistency + MM budgets). |
| **MilpSolver** | `milp.rs` | MIQCQP via SCIP (russcip). Exact optimal with timeout. Feature-gated: `milp`. |

## Pipeline Architecture

Pipelines are built via `PipelineBuilder` and run phases in sequence:

```
Pipeline::current()         → LocalSolver → MmAllocator
Pipeline::with_negrisk()    → Fixed-point: LocalSolver → NegriskSolver → MmAllocator
Pipeline::with_dual_decomposition() → DualMaster → MultiMarketSolver
```

The pipeline can iterate (fixed-point) until welfare converges.

## Trait Hierarchy

| Trait | Methods | Implementors |
|-------|---------|--------------|
| `PriceDiscoverer` | `discover_prices() → PriceDiscoveryResult` | LocalSolver |
| `OrderAllocator` | `allocate() → AllocationResult` | MmAllocator |
| `PartialSolver` | `solve_partial() → PartialSolution` | MilpSolver |
| `Solver` | `solve() → MatchingResult` | Pipeline (legacy) |

## MILP Formulation (milp.rs)

Variables:
- `z_i ∈ {0,1}` — fill indicator
- `q_i` — fill quantity
- `p_m` — clearing price per market
- `mint_m` — per-market minting
- `group_mint_g` — group-level minting (key for negrisk-style arbitrage)

Constraints:
- UCP via Big-M
- Position balance (YES/NO nets to mint)
- MM budget (bilinear `p × q` via SCIP MIQCQP or McCormick linearization)
- Market group price sums ≤ $1

## Key Files

| File | Lines | Purpose |
|------|-------|---------|
| `pipeline.rs` | ~800 | Pipeline orchestration, builder pattern |
| `local_solver.rs` | ~500 | Binary unified clearing |
| `milp.rs` | ~900 | SCIP-based MIQCQP solver |
| `mm_allocator.rs` | ~400 | Budget-constrained allocation |
| `dual_master.rs` | ~600 | Dual decomposition |
| `verifier.rs` | ~400 | Result verification for ZK integration |
| `combiner/mwis.rs` | ~300 | Maximum Weight Independent Set for solution combining |

## Testing

```bash
cargo test -p matching-solver                    # All tests
cargo test -p matching-solver --features milp   # Include MILP tests
cargo test -p matching-solver test_milp         # MILP-specific tests
```

## Performance Notes

- LocalSolver: O(n log n) via sorting
- NegriskSolver: O(G × M) where G = groups, M = markets per group
- MilpSolver: Time-limited (default 5s), reports gap when timeout
