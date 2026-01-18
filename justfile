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

# Run realistic scenario (small/test version)
realistic-small:
    cargo run --bin matching-sim --release -- --scenario realistic-test

# Run realistic scenario (full)
realistic:
    cargo run --bin matching-sim --release -- --scenario realistic

# Run all solvers comparison
compare:
    cargo run --bin matching-sim --release -- --scenario milp-killer --solver all

# Run specific solver on a scenario
run scenario solver="greedy":
    cargo run --bin matching-sim --release -- --scenario {{scenario}} --solver {{solver}}

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
