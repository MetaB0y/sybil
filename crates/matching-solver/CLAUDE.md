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
Pipeline::current()         → with_dual_decomposition()
Pipeline::with_negrisk()    → Fixed-point: LocalSolver → NegriskSolver → MmAllocator
Pipeline::with_dual_decomposition() → DualMaster → MultiMarketSolver
```

The pipeline can iterate (fixed-point) until welfare converges.

## Pipeline Flow (Dual Decomposition)

`Pipeline::current()` = `with_dual_decomposition()`. This is the production path.

1. **DualMaster** runs multiple iterations internally, shading orders with Lagrangian multipliers (λ) to enforce price consistency across market groups.
2. Each iteration: LocalSolver clears each market → greedy MM knapsack allocates budget → fills accumulated, filled orders removed.
3. Price merge: only markets with `has_activity == true` update prices. Markets without activity retain prices from earlier iterations (prevents 50/50 overwrite bug).
4. After convergence (or max iterations), partial solvers (MILP if enabled) handle remaining multi-market orders.
5. **`enforce_ucp`** runs AFTER the pipeline, re-pricing all single-market binary fills at the final clearing prices. Three sub-phases:
   - `reprice_and_filter_fills`: re-price at final prices, drop limit-violating fills
   - `trim_position_imbalance`: ensure YES qty == NO qty per market (trim lowest welfare first)
   - `collect_final_fills`: filter zero-qty, recompute stats
6. If total welfare is negative after UCP enforcement, the result is cleared (no fills).

**Important**: `MarketSolution::empty()` has `has_activity: false` — never treat its prices as real market signals.

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
