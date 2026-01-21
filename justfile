# Justfile for matching simulation workspace

# Default recipe - show available commands
default:
    @just --list

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    cargo clippy --workspace --all-features

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Quick simulation (~50 orders)
sim-quick:
    cargo run --bin matching-sim --release -- --preset quick

# Small simulation (~300 orders)
sim-small:
    cargo run --bin matching-sim --release -- --preset small

# Medium simulation (~3000 orders)
sim-medium:
    cargo run --bin matching-sim --release -- --preset medium

# Large simulation (~10000 orders)
sim-large:
    cargo run --bin matching-sim --release -- --preset large

# Compare all solvers on medium scenario
compare:
    cargo run --bin matching-sim --release -- --preset medium --solver all

# MILP-killer test (forces MILP timeout)
milp-killer:
    cargo run --bin matching-sim --release -- --preset milp-killer --solver all --milp-timeout 5.0

# Run with specific preset and solver
sim preset="medium" solver="greedy":
    cargo run --bin matching-sim --release -- --preset {{preset}} --solver {{solver}}

# Build release
build:
    cargo build --release

# Clean and rebuild
rebuild:
    cargo clean && cargo build --release

# Build documentation
doc:
    cargo doc --workspace --no-deps

# Open documentation in browser
doc-open:
    cargo doc --workspace --no-deps --open

# Check all (compile, test, lint, fmt)
check-all: fmt-check lint test
    @echo "All checks passed!"

# Run benchmarks if any
bench:
    cargo bench --workspace

# Watch for changes and run tests
watch:
    cargo watch -x "test --workspace"

# Clean build artifacts
clean:
    cargo clean

# Show dependency tree
deps:
    cargo tree --workspace

# Update dependencies
update:
    cargo update
