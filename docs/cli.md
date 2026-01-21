# CLI Usage

## matching-sim

The main CLI tool for running matching simulations.

### Basic Usage

```bash
# Show help
cargo run --bin matching-sim -- --help

# Run quick test
cargo run --bin matching-sim -- --quick

# Run specific scenario
cargo run --bin matching-sim -- --scenario random-easy
```

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `--scenario <S>` | Scenario to run | random-* |
| `--solver <S>` | Solver to use | greedy |
| `--batches <N>` | Batches per scenario | 20 |
| `--seed <N>` | Random seed | 42 |
| `--milp-timeout <S>` | MILP time limit (seconds) | none |
| `--verbose, -v` | Detailed output | false |

### Solver Options

- `greedy` - Fast heuristic
- `milp` - Optimal via MILP
- `pipeline` - Full pipeline (price discovery + projection + MM allocation)
- `all` - Compare all solvers

### Examples

```bash
# Compare all solvers on a scenario
cargo run --bin matching-sim --release -- \
    --scenario random-hard \
    --solver all \
    --batches 5

# Run MILP with timeout
cargo run --bin matching-sim --release -- \
    --scenario mega-medium \
    --solver milp \
    --milp-timeout 10

# Verbose output
cargo run --bin matching-sim --release -- \
    --scenario random-hard \
    --solver pipeline \
    --verbose

# Multiple batches for statistics
cargo run --bin matching-sim --release -- \
    --scenario milp-killer \
    --solver all \
    --batches 50
```

### Specialized Tests

```bash
# MILP killer test
cargo run --bin matching-sim --release -- --milp-killer --config full
```

## Justfile Commands

The project includes a justfile for common operations:

```bash
# List all commands
just

# Run tests
just test

# Run clippy
just lint

# Format code
just fmt

# Compare solvers
just compare
```

## Output Format

### Solver Comparison Table

```
Scenario: random-hard

╭────────────┬─────────────┬──────────┬──────────╮
│ Solver     │ Welfare     │ Gap      │ Fill %   │
├────────────┼─────────────┼──────────┼──────────┤
│ MILP       │    12345678 │ 0.0%     │   95.2%  │
│ Pipeline   │    12340000 │ 0.1%     │   94.8%  │
│ Greedy     │    11000000 │ 10.9%    │   89.0%  │
╰────────────┴─────────────┴──────────┴──────────╯
```

- **Welfare**: Total welfare achieved
- **Gap**: Percentage below best result
- **Fill %**: Percentage of orders filled

### Verbose Output

With `--verbose`:
```
Running random-hard batch 0 (seed 42)
Problem: Random(hard)
  Markets: 50, Orders: 500, Liquidity entries: 200

  MILP: welfare=12345678, filled=480/500, time=0.234s
  Greedy: welfare=11000000, filled=450/500, time=0.001s
```

## Exit Codes

- `0` - Success
- `1` - Error (invalid arguments, scenario not found, etc.)

## Environment Variables

None currently used. All configuration via CLI arguments.

## Performance Tips

1. **Use release builds** for benchmarking:
   ```bash
   cargo run --bin matching-sim --release -- ...
   ```

2. **Adjust MILP timeout** based on problem size:
   - Small (< 100 orders): 1-5s
   - Medium (100-1000): 5-30s
   - Large (> 1000): 30-120s or skip MILP

3. **Use pipeline solver** for production:
   ```bash
   cargo run --bin matching-sim --release -- \
       --solver pipeline \
       --milp-timeout 5
   ```

4. **Increase batches** for statistical significance:
   ```bash
   cargo run --bin matching-sim --release -- \
       --batches 100 \
       --solver all
   ```
