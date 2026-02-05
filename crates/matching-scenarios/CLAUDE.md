# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **matching-scenarios** crate generates synthetic prediction market order books for testing and benchmarking. It provides reproducible scenario generation with configurable complexity.

## Primary API

```rust
use matching_scenarios::{generate_scenario, ScenarioConfig};

let problem = generate_scenario(ScenarioConfig::medium());
```

## Presets

| Preset | Orders | Markets | Bundles | Spreads | AON | MMs | Use Case |
|--------|--------|---------|---------|---------|-----|-----|----------|
| `quick()` | ~50 | 5 | 10% | 0% | 0% | 0 | Unit tests, rapid iteration |
| `small()` | ~300 | 10 | 15% | 5% | 5% | 1 | Local development |
| `medium()` | ~3000 | 30 | 15% | 5% | 10% | 2 | Integration tests, CI |
| `large()` | ~10k | 50 | 20% | 5% | 15% | 3 | Performance benchmarks |
| `extreme()` | ~100k | 200 | 20% | 5% | 15% | 10 | Scaling limits |
| `milp_killer()` | ~5k | 50 | 30% | 0% | 45% | 0 | Force MILP timeout |

## ScenarioConfig Fields

**Market Configuration:**
- `num_markets` — number of binary markets
- Markets are grouped into 3-4 mutually exclusive sets ~60% of the time

**Order Configuration:**
- `num_orders` — total orders to generate
- `bundle_fraction` — fraction spanning multiple markets
- `spread_fraction` — fraction that are spread trades
- `aon_fraction` — fraction with all-or-none constraint
- `order_size_min/max` — quantity range
- `seed` — ChaCha8 seed for reproducibility

**Liquidity Configuration:**
- `liquidity_scarcity` (0.0-1.0) — lower = scarcer = tighter matching
- `hot_market_fraction` — markets receiving extra demand

**Market Maker Configuration:**
- `num_mms` — number of MM constraints
- `mm_budget_min/max` — budget range in dollars
- `mm_spread_bps` — spread in basis points

## Generation Process

1. Create N binary markets with fair prices (normal distribution around $0.50)
2. Group ~60% into mutually exclusive sets (multi-outcome events)
3. Add liquidity depth (3 price levels × 2 outcomes × 2 sides per market)
4. Generate orders: simple (~30-95%), bundles (15-30%), spreads (5-20%)
5. Apply AON constraints based on config
6. Add MM constraints with aggressive quotes (willing to cross spreads)
7. Shuffle orders to avoid order-dependent solver behavior

## Reproducibility

All scenarios use seeded RNGs (ChaCha8Rng). Same seed = same Problem.

```rust
let config = ScenarioConfig { seed: 12345, ..ScenarioConfig::medium() };
let p1 = generate_scenario(config.clone());
let p2 = generate_scenario(config);
assert_eq!(p1.orders.len(), p2.orders.len()); // Deterministic
```

## Module Structure

| Module | Purpose |
|--------|---------|
| `scenario.rs` | Primary generator with `ScenarioConfig` |
| `random.rs` | Simpler `RandomConfig` for basic testing |
