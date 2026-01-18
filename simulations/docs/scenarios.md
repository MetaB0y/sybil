# Scenarios

Test scenarios for benchmarking and validating solvers.

## Standard Scenarios

### presidential

Election prediction market with:
- 5 candidate markets (binary: wins/loses)
- Simple orders betting on individual candidates
- Bundle orders betting on combinations
- Mutual exclusion constraints (only one winner)

```bash
cargo run --bin matching-sim --release -- --scenario presidential
```

### presidential-hard

Harder version with more orders and less liquidity.

### tournament

Sports tournament with elimination bracket:
- 8 teams, bracket structure
- Implication constraints (winner of A vs B must win semifinal to win final)
- Orders at different bracket levels

### tournament-large

16 teams, more orders per team.

### random-easy / random-medium / random-hard

Random scenarios with varying complexity:
- easy: 10 markets, 100 orders, low bundle fraction
- medium: 20 markets, 200 orders, medium bundles
- hard: 50 markets, 500 orders, high bundles

## Complex Scenarios

### nested-bundles

Orders with deeply nested bundle structures:
- Bundles of bundles
- Tests solver handling of complex payoffs

### conditional-chains

Price-triggered conditional orders:
- Order A activates if market X price > threshold
- Tests ConditionalEvaluator

### deep-implications

Long implication chains (A -> B -> C -> D -> E):
- Tests constraint propagation
- ChainFinder should excel here

### liquidity-cliffs

Markets with sudden liquidity drops:
- Abundant liquidity until certain price
- Then very sparse
- Tests partial fill handling

### adversarial

Specifically designed to confuse greedy:
- Orders that look good individually but conflict
- Optimal requires coordinated selection

### large-interconnected

Many orders sharing markets:
- High degree of conflict
- Tests conflict resolution

## Stress Scenarios

### mega-small / mega-medium / mega-large / mega-extreme

Scalability testing:
- small: 50 markets, 1K orders
- medium: 100 markets, 5K orders
- large: 200 markets, 10K orders
- extreme: 500 markets, 50K orders

### combined

Combination of multiple scenario types.

### milp-killer / milp-killer-full / milp-killer-extreme

Designed to force MILP timeout:
- High branching factor
- Many symmetric solutions
- Tests fallback behavior

## Planted Scenarios

Known patterns for testing specific solver capabilities.

### planted-chain

Hidden implication chain with mispriced liquidity:
- Greedy fills expensive end
- ChainFinder should find cheap end
- Demonstrates constraint exploitation value

### planted-complement

Complete covering bundle set:
- 4 bundles covering all states
- Combined cost < guaranteed payout
- BundleDecomposer should find this

### planted-exclusion

Mutual exclusion groups with mispricing:
- Tests exclusion constraint handling

## Realistic Scenarios

Production-like order distributions.

### realistic-test (10K orders)

Quick test configuration:
```bash
just realistic-small
```

### realistic-small (3K orders)

Smaller realistic scenario.

### realistic / realistic-standard (50K orders)

Full realistic scenario:
```bash
just realistic
```

### realistic-extreme (100K orders)

Maximum scale realistic scenario.

### realistic-cross-market

High bundle order fraction:
- Demonstrates cross-market matching value
- MILP should beat greedy significantly

## Configuration Options

All scenarios accept a seed parameter for reproducibility:

```rust
let config = PresidentialConfig {
    seed: 42,
    num_simple_orders: 30,
    num_bundle_orders: 10,
    liquidity_multiplier: 0.5,
    ..Default::default()
};
let problem = generate_presidential_scenario(config);
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
| Quick sanity check | presidential, random-easy |
| Solver comparison | milp-killer, adversarial |
| Scalability testing | mega-*, realistic-* |
| Specialized solver testing | planted-* |
| Production simulation | realistic-standard |
