> **Superseded.** The canonical architecture spec now lives in the Obsidian vault at `docs/architecture/`. This file is kept for historical reference. Entry point: `docs/architecture/Sybil Architecture.md`.

# Sybil: Architecture

## Overview

Sybil is a **prediction market matching engine** built on **Frequent Batch Auctions (FBA)**. It solves the welfare-maximizing clearing problem — given orders with complex payoff structures across multiple markets and budget-constrained market makers, find fills and prices that maximize total surplus.

The theoretical foundation is established in two companion papers:
- **paper.typ** (canonical repo `~/github/prediction-markets-are-fisher-markets/`, was `lmsr-proof.typ`): Prediction markets are Fisher markets. Risk-averse (quasi-linear EG) clearing is a convex program with unique prices and implicit budget constraints.
- **decomposition.typ** (same canonical repo): Budget decomposition across market groups via mirror descent.

---

## Solvers

All solvers take a `Problem` and return a `PipelineResult` (fills, clearing prices, welfare, timing). Feature-gated.

| Solver | File | Feature | Objective | Description |
|--------|------|---------|-----------|-------------|
| **LpSolver** | `lp_solver.rs` | `lp` | Linear welfare | LP via HiGHS. Heuristic budget shading (SLP). Fast. |
| **EgSolver** | `eg_solver.rs` | `lp` | Fisher (B_k ln U_k) | Frank-Wolfe on EG program. No cash variable. |
| **ConicSolver** | `conic_solver.rs` | `conic` | Configurable | Clarabel interior-point. Supports three modes via `ObjectiveMode`. |
| **MilpSolver** | `milp.rs` | `milp` | Linear welfare | SCIP MIQCQP. Exact optimal with timeout. |
| **DecomposedSolver** | `decomposed.rs` | `lp` | (wraps inner) | Partitions by market group, coordinates MM budgets via mirror descent. |

### ConicSolver Modes

The conic solver (`ObjectiveMode`) unifies all three objective formulations:

| Mode | Objective | Budget handling | Use case |
|------|-----------|-----------------|----------|
| `Linear` | max Σ w_j q_j | Delegates to LpSolver | Risk-neutral baseline |
| `Fisher` | max Σ B_k ln(U_k) + Σ w_j q_j | Absorbed via log | No cash variable — MMs may get forced fills |
| `QuasiFisher` | max Σ [B_k ln(U_k+s_k) - s_k] + Σ w_j q_j | Absorbed via log + cash | **Default.** Paper's Theorem 5. μ_k ≤ 1 guaranteed. |

The `temperature` parameter (LMSR smoothing, b > 0) is reserved for future work.

### Decomposed Solving

`DecomposedSolver<S>` wraps any `ComponentSolver` (LP, EG, or Conic) and:
1. Partitions the problem by connected market groups
2. Solves each component independently
3. Coordinates MM budgets across components via multiplicative-weights (mirror descent)

This is the computational payoff of the Fisher market structure — with log utility, budget allocation is smooth and concave. With linear welfare, it's combinatorial. See `decomposition.typ` §2-3 in `~/github/prediction-markets-are-fisher-markets/` (`design/math-papers.md`).

---

## Key Abstractions

### Problem

`matching-engine::Problem` — the input to all solvers:
- `orders: Vec<Order>` — payoff vectors over market states
- `markets: MarketRegistry` — binary outcome markets
- `market_groups: Vec<MarketGroup>` — mutually exclusive groups (enables group minting)
- `mm_constraints: Vec<MmConstraint>` — per-MM budget caps and order assignments

### PipelineResult

`matching-solver::PipelineResult` — the output from all solvers:
- `result: MatchingResult` — fills, welfare, volume
- `price_discovery: PriceDiscoveryResult` — clearing prices per market
- Timing data and phase breakdowns

### Verification

`sybil-verifier` validates solver output independently:
- Order constraints (quantity, price limits)
- Settlement-derived MINT account adjustments (no phantom shares)
- MM budget compliance
- Welfare computation correctness

Designed for ZK proof integration — the verifier checks a `BlockWitness` that encodes all information needed to verify a batch.

---

## Crate Map

| Crate | Purpose |
|-------|---------|
| `matching-engine` | Core types: orders, fills, markets, payoff vectors, MM constraints |
| `matching-solver` | Solver implementations (LP, EG, Conic, MILP, Decomposed) |
| `matching-scenarios` | Test scenario generators |
| `matching-sim` | CLI simulation tool with presets and solver comparison |
| `matching-sequencer` | Agent-based multi-batch sequential simulation |
| `sybil-api` | HTTP API server for agent trading |
| `sybil-oracle` | Oracle/resolution service |
| `sybil-verifier` | ZK-ready block verification |

---

## Design Principles

1. **All integer arithmetic**: Prices/quantities in nanos (10^9 per dollar). No floating point in the engine.
2. **Payoff vectors**: Orders are payoff vectors over market states, enabling unified handling of simple orders, bundles, spreads, and conditionals.
3. **Welfare maximization**: Objective is `Σ (limit_price - clearing_price) × fill_qty`, not volume.
4. **Fisher market structure**: MM budgets absorbed into the EG objective (no explicit budget constraints). See `paper.typ` Theorem 5 in `~/github/prediction-markets-are-fisher-markets/` (`design/math-papers.md`).
5. **Verifiability**: All results verified by an independent checker (ZK-ready).
