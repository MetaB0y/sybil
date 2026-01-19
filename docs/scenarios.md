# Scenarios

Test scenarios for benchmarking and validating solvers.

## Standard Scenarios

### random-easy / random-medium / random-hard

Random scenarios with varying complexity:
- easy: 10 markets, 100 orders, low bundle fraction
- medium: 20 markets, 200 orders, medium bundles
- hard: 50 markets, 500 orders, high bundles

```bash
cargo run --bin matching-sim --release -- --scenario random-easy
cargo run --bin matching-sim --release -- --scenario random-hard
```

## Stress Scenarios

### mega-small / mega-medium / mega-large / mega-extreme

Scalability testing:
- small: 20 markets, 500 orders
- medium: 30 markets, 1K orders
- large: 50 markets, 2K orders
- extreme: 75 markets, 5K orders

### combined

Combination of multiple random scenario types with varying configurations.

### milp-killer / milp-killer-full / milp-killer-extreme

Designed to force MILP timeout:
- High branching factor
- Many symmetric solutions
- Tests fallback behavior

## Configuration Options

All scenarios accept a seed parameter for reproducibility:

```rust
let config = RandomConfig {
    seed: 42,
    num_markets: 20,
    num_orders: 200,
    bundle_fraction: 0.3,
    ..RandomConfig::medium()
};
let problem = generate_random_scenario(config);
```

## Adding New Scenarios

1. Add config struct to `matching-scenarios/src/`
2. Implement generator function
3. Export from `lib.rs`
4. Add case to `matching-sim/src/scenarios.rs`
5. Document here

## Scenario Selection Guide

| Goal | Recommended Scenarios |
|------|----------------------|
| Quick sanity check | random-easy |
| Solver comparison | milp-killer, random-hard |
| Scalability testing | mega-* |
| Production simulation | mega-medium |
