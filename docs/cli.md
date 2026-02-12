# CLI Usage

## matching-sim

The main CLI tool for running matching simulations.

### Quick Start

```bash
# Show help
cargo run --bin matching-sim --release -- --help

# Run with preset
just sim-quick     # ~50 orders
just sim-small     # ~300 orders
just sim-medium    # ~3000 orders
just sim-large     # ~10000 orders
just sim-extreme   # ~100k orders

# Compare solvers
just compare
```

### Presets

| Preset | Orders | Use Case |
|--------|--------|----------|
| `quick` | ~50 | Fast iteration, debugging |
| `small` | ~300 | Basic testing |
| `medium` | ~3000 | Standard benchmark |
| `large` | ~10000 | Performance testing |
| `extreme` | ~100k | Stress testing |
| `milp-killer` | Forces MILP timeout | Solver comparison |

### Options

```
Presets:
  --preset <NAME>      quick, small, medium, large, extreme, milp-killer

Custom configuration:
  --markets <N>        Number of markets
  --orders <N>         Number of orders
  --bundles <F>        Bundle fraction (0.0-1.0)
  --spreads <F>        Spread fraction (0.0-1.0)
  --scarcity <F>       Liquidity scarcity (0.0-1.0, lower=scarcer)
  --mms <N>            Number of market makers

Solver options:
  --solver <S>         pipeline (default), greedy, milp, all
  --milp-timeout <S>   MILP time limit in seconds

Other options:
  --batches <N>        Number of batches to run (default: 1)
  --seed <N>           Random seed (default: 42)
  --verbose, -v        Show detailed step-by-step output
```

### Examples

```bash
# Run medium scenario with detailed output
cargo run --bin matching-sim --release -- --preset medium -v

# Compare all solvers
cargo run --bin matching-sim --release -- --preset medium --solver all

# Custom configuration
cargo run --bin matching-sim --release -- \
    --markets 50 \
    --orders 2000 \
    --bundles 0.15 \
    --mms 5 \
    --solver pipeline \
    -v

# MILP with timeout
cargo run --bin matching-sim --release -- \
    --preset large \
    --solver milp \
    --milp-timeout 10
```

## Justfile Commands

```bash
just                 # List all commands

# Simulation presets
just sim-quick       # ~50 orders, verbose
just sim-small       # ~300 orders, verbose
just sim-medium      # ~3000 orders, verbose
just sim-large       # ~10000 orders, verbose
just sim-extreme     # ~100k orders, verbose

# Comparison
just compare         # Compare solvers on medium
just milp-killer     # MILP stress test

# Development
just test            # Run all tests
just lint            # Run clippy
just fmt             # Format code
just bench           # Run benchmarks
just check-all       # fmt + lint + test

# Custom
just sim medium pipeline -v   # Preset + solver + verbose flag
```
