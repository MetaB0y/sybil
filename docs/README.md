# Sybil V2: Documentation

Sybil V2 is a prediction market matching engine using Frequent Batch Auctions (FBA) with support for cross-market orders and linear constraint-based order representation.

## Documentation Index

### Core Architecture
- **[architecture.md](./architecture.md)** - System design, key decisions, complexity analysis
- **[matching-algorithm.md](./matching-algorithm.md)** - Patch-based cross-market solving algorithm
- **[order-types.md](./order-types.md)** - Supported order types and their LP representations

### JIT Liquidity
- **[jit-design.md](./jit-design.md)** - Just-In-Time liquidity mechanism design

### Planning & Status
- **[next-steps.md](./next-steps.md)** - Implementation roadmap and priorities

---

## Quick Overview

### What's Implemented

The `simulations/` crate workspace contains:

| Crate | Purpose | Status |
|-------|---------|--------|
| `matching-engine` | Core types, orders, fills, liquidity pools | Complete |
| `matching-solver` | Solvers (Greedy, MILP, Randomized, Platform) | Complete |
| `matching-scenarios` | Test scenarios (presidential, realistic, stress) | Complete |
| `matching-sim` | CLI simulation tool | Complete |
| `jit-study` | JIT liquidity research simulations | Experimental |

### Key Design Decisions

1. **Linear constraint orders** - Orders are LP constraints, not simple limit orders
2. **Patch-based solving** - Solve single markets first, then apply cross-market patches
3. **MWIS combination** - Multiple solver solutions combined via Maximum Weight Independent Set
4. **Uniform clearing price** - All fills in a market clear at the same price
5. **Integer arithmetic** - i64/u64 for all amounts and prices (nanos = 10^-9)

### Running the Simulation

```bash
cd simulations
cargo run --release -- --scenario presidential --solver platform
```

See `matching-sim --help` for all options.
