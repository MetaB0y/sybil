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

# Quick simulation with detailed output (~50 orders)
sim-quick:
    cargo run --bin matching-sim --release -- --preset quick -v

# Small simulation with detailed output (~300 orders)
sim-small:
    cargo run --bin matching-sim --release -- --preset small -v

# Medium simulation with detailed output (~3000 orders)
sim-medium:
    cargo run --bin matching-sim --release -- --preset medium -v

# Large simulation with detailed output (~10000 orders)
sim-large:
    cargo run --bin matching-sim --release -- --preset large -v

# Extreme simulation with detailed output (~100k orders)
sim-extreme:
    cargo run --bin matching-sim --release -- --preset extreme -v

# Compare all solvers on a scenario
compare preset="medium":
    cargo run --bin matching-sim --release --features lp -- --preset {{preset}} --solver all

# MILP-killer test (forces MILP timeout)
milp-killer:
    cargo run --bin matching-sim --release -- --preset milp-killer --solver all --milp-timeout 5.0

# Run with specific preset and solver
sim preset="medium" solver="pipeline" verbose="-v":
    cargo run --bin matching-sim --release -- --preset {{preset}} --solver {{solver}} {{verbose}}

# Run with LP solver
sim-lp preset="quick":
    cargo run --bin matching-sim --release --features lp -- --preset {{preset}} --solver lp -v

# Compare LP against all solvers
compare-lp preset="medium":
    cargo run --bin matching-sim --release --features lp -- --preset {{preset}} --solver all -v

# Run with negrisk arbitrage solver
sim-negrisk preset="medium":
    cargo run --bin matching-sim --release --features viz -- --preset {{preset}} --solver negrisk -v

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

# Export JSON snapshot
export-json preset="medium" output="/tmp/snapshot.json":
    cargo run --bin matching-sim --release -- --preset {{preset}} --export-json {{output}} -v

# Show ASCII convergence charts
sim-charts preset="small":
    cargo run --bin matching-sim --release -- --preset {{preset}} --show-charts -v

# Export JSON and show charts
sim-viz preset="medium" output="/tmp/snapshot.json":
    cargo run --bin matching-sim --release -- --preset {{preset}} --export-json {{output}} --show-charts -v

# Run visualization dashboard
viz snapshot="/tmp/snapshot.json":
    cd viz && uv run streamlit run app.py -- {{snapshot}}

# Generate snapshot and launch visualization in one command
viz-run preset="small":
    cargo run --bin matching-sim --release --features viz -- --preset {{preset}} --export-json /tmp/snapshot.json
    cd viz && uv run streamlit run app.py -- /tmp/snapshot.json

# Install viz dependencies
viz-install:
    cd viz && uv sync

# Run arena demo (starts server, syncs deps, runs backtest)
arena-demo:
    cd arena && uv sync --extra llm && uv run python demo.py

# Run arena demo without starting server (server must already be running)
arena-demo-quick:
    cd arena && uv run python demo.py
