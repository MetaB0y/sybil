# matching-scenarios

Test scenario generation for the matching solver.

## Purpose

Generates synthetic `Problem` instances for testing, benchmarking, and validation.
Provides configurable scenarios from small (unit tests) to extreme (stress tests).

## Key Components

### MegaScenarioConfigV2 (`mega.rs`)
Main configuration for realistic scenario generation:
- `num_markets`: Number of markets
- `orders_per_market`: Order density
- `bundle_fraction`: Fraction of orders that are cross-market bundles
- `mm_count`: Number of market makers with budget constraints

### Preset Configurations
- `small()`: ~300-500 orders (quick unit tests)
- `medium()`: ~3,000-5,000 orders (normal testing)
- `large()`: ~20,000-30,000 orders (stress testing)
- `extreme()`: ~70,000-100,000 orders (extreme stress)

## Usage

```rust
use matching_scenarios::{generate_mega_scenario_v2, MegaScenarioConfigV2};

let problem = generate_mega_scenario_v2(MegaScenarioConfigV2::medium());
```

## Generated Content

- Binary and multi-outcome markets
- Single-market limit orders (YES/NO buys and sells)
- Cross-market bundle orders
- Market maker liquidity
- MM budget constraints
- Market constraints (implications)

## Dependencies

- `matching-engine`: Core types
- `rand`, `rand_chacha`: Deterministic randomness
