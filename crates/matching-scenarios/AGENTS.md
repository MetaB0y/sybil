# matching-scenarios

Test scenario generation for the matching solver.

## Purpose

Generates synthetic `Problem` instances for testing, benchmarking, and validation.
Provides configurable scenarios from small (unit tests) to extreme (stress tests).
All generated orders are single-market (binary YES/NO).

## Key Components

### ScenarioConfig (`scenario.rs`)
Main configuration for scenario generation:
- `num_markets`: Number of binary markets
- `num_orders`: Total single-market orders
- `num_mms`: Number of market makers with budget constraints

### Preset Configurations
- `quick()`: ~50 orders (rapid iteration)
- `small()`: ~300 orders (unit tests)
- `medium()`: ~3,000 orders (integration tests)
- `large()`: ~10,000 orders (stress testing)
- `extreme()`: ~100,000 orders (scaling limits)

## Usage

```rust
use matching_scenarios::{generate_scenario, ScenarioConfig};

let problem = generate_scenario(ScenarioConfig::medium());
```

## Generated Content

- Binary and multi-outcome markets (grouped into mutually exclusive sets)
- Single-market limit orders (YES/NO buys and sells)
- Market maker liquidity ladders
- MM budget constraints

## Dependencies

- `matching-engine`: Core types
- `rand`, `rand_chacha`: Deterministic randomness
