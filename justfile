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
    cargo run --bin matching-sim --release -- --preset {{preset}} --solver all

# MILP-killer test (forces MILP timeout)
milp-killer:
    cargo run --bin matching-sim --release -- --preset milp-killer --solver all --milp-timeout 5.0

# Run with specific preset and solver
sim preset="medium" solver="lp" verbose="-v":
    cargo run --bin matching-sim --release -- --preset {{preset}} --solver {{solver}} {{verbose}}

# Build release
build:
    cargo build --release

# Install OpenVM 2.0 beta CLI used by the ZK guest tooling
openvm-install:
    cargo install --locked --git https://github.com/openvm-org/openvm.git --tag v2.0.0-beta.2 cargo-openvm

# Check the OpenVM guest crate with the normal host compiler
openvm-guest-check:
    cargo check --manifest-path zk/openvm-guest/Cargo.toml

# Build and transpile the Sybil OpenVM guest
openvm-guest-build:
    cargo openvm build --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil

# Inspect a serialized state-transition proof job
prover-inspect job:
    cargo run -p sybil-prover -- inspect --job {{job}}

# Validate a proof job and emit a serialized OpenVM guest input artifact
prover-prepare job guest_input="/tmp/sybil-guest-input.msgpack" public_input_hash="/tmp/sybil-public-input-hash.hex":
    cargo run -p sybil-prover -- prepare --job {{job}} --guest-input {{guest_input}} --public-input-hash {{public_input_hash}}

# Export the latest committed sequencer block as a portable proof job
witgen-export-latest store job="/tmp/sybil-proof-job.msgpack":
    cargo run -p sybil-witgen-cli -- export-latest --store {{store}} --job {{job}}

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
check-all: fmt-check lint test contracts-fmt-check contracts-build contracts-test
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

# Run EG (Eisenberg-Gale / Fisher market) solver
sim-eg preset="quick":
    cargo run --bin matching-sim --release --features lp -- --preset {{preset}} --solver eg -v

# Run arena demo (starts server, syncs deps, runs backtest)
arena-demo:
    cd arena && uv sync --extra llm && uv run python demo.py

# Run arena demo without starting server (server must already be running)
arena-demo-quick:
    cd arena && uv run python demo.py

# ── Architecture Vault ───────────────────────────────────────────────────────

# Validate vault (links, frontmatter, staleness, orphans)
docs-check:
    ./scripts/check-vault.sh

# List notes with last_verified > 90 days
docs-stale:
    @for f in docs/architecture/*.md; do \
        lv="$(awk '/^---$/{n++; next} n==1 && /^last_verified:/{print $2; exit}' "$f")"; \
        [ -z "$lv" ] && continue; \
        days=$(( ($(date +%s) - $(date -d "$lv" +%s 2>/dev/null || echo $(date +%s))) / 86400 )); \
        [ "$days" -gt 90 ] && echo "  $days days: $(basename "$f" .md) (last: $lv)"; \
    done; true

# Search vault content
docs-search term:
    @grep -rni "{{term}}" docs/architecture/*.md --include='*.md' | sed 's|docs/architecture/||'

# List all notes with layer + status
docs-list:
    @for f in docs/architecture/*.md; do \
        layer="$(awk '/^---$/{n++; next} n==1 && /^layer:/{print $2; exit}' "$f")"; \
        status="$(awk '/^---$/{n++; next} n==1 && /^status:/{print $2; exit}' "$f")"; \
        printf "  %-12s %-12s %s\n" "$layer" "$status" "$(basename "$f" .md)"; \
    done

# Rename note and update wiki-links (requires notesmd-cli)
docs-rename old new:
    notesmd-cli move "{{old}}" "{{new}}" --vault docs/architecture

# Print note with incoming mentions (requires notesmd-cli)
docs-read note:
    notesmd-cli print "{{note}}" --vault docs/architecture --mentions

# Set last_verified to today (requires notesmd-cli)
docs-verify note:
    notesmd-cli frontmatter "{{note}}" --vault docs/architecture --edit --key last_verified --value "$(date +%Y-%m-%d)"

# Pre-commit check (Rust fmt/clippy + Solidity fmt)
pre-commit: contracts-fmt-check
    cargo fmt --all -- --check
    cargo clippy --workspace --all-features

# E2E smoke test (starts server, exercises API, tears down)
smoke:
    ./scripts/smoke-test.sh

# ── Docker ─────────────────────────────────────────────────────────────────

# Build Docker image
docker-build:
    docker compose build

# Start all services (API + VictoriaMetrics + Tempo + Grafana)
docker-up:
    docker compose up -d

# Stop all services
docker-down:
    docker compose down

# Tail API logs
docker-logs:
    docker compose logs -f sybil-api

# ── Polymarket Mirror ──────────────────────────────────────────────────────

# Run Polymarket mirror (sybil-api must be running in dev-mode)
polymarket max_events="10":
    cargo run --release -p sybil-polymarket -- --max-events {{max_events}}

# Run Polymarket mirror with custom Sybil URL
polymarket-dev port="3001" max_events="10":
    cargo run --release -p sybil-polymarket -- --sybil-url http://localhost:{{port}} --max-events {{max_events}} --mm-half-spread 0.03

# ── Deploy (SSH) ──────────────────────────────────────────────────────────
# Production uses the same docker-compose.yml as local dev, with
# docker-compose.prod.yml layered on top (persistence, prod params, Caddy).
# docker-compose.override.yml (build contexts) is NOT shipped to the server.

SERVER := "root@172.104.31.54"
COMPOSE_PROD := "docker compose -f docker-compose.yml -f docker-compose.prod.yml"

# Sync compose configs + deploy/ directory to server
deploy-sync:
    ssh {{SERVER}} 'mkdir -p /opt/sybil'
    scp docker-compose.yml docker-compose.prod.yml {{SERVER}}:/opt/sybil/
    scp -r deploy {{SERVER}}:/opt/sybil/

# Build and deploy sybil-api + polymarket mirror
deploy-api: deploy-sync
    docker compose build sybil-api
    docker save sybil-api:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d sybil-api sybil-polymarket'

# Build and deploy arena bots + dashboard (pass OpenRouter key)
deploy-arena key: deploy-sync
    docker compose build sybil-arena
    docker save sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && OPENROUTER_API_KEY={{key}} {{COMPOSE_PROD}} up -d sybil-arena sybil-arena-dashboard caddy'

# Deploy observability stack (VictoriaMetrics + Tempo + Grafana)
deploy-monitoring: deploy-sync
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d victoriametrics tempo grafana'

# Deploy Caddy HTTPS reverse proxy
deploy-caddy: deploy-sync
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d caddy'

# Deploy everything
deploy-all key: deploy-sync
    docker compose build
    docker save sybil-api:latest sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && OPENROUTER_API_KEY={{key}} {{COMPOSE_PROD}} up -d --remove-orphans'

# Tail logs from a container on the server
deploy-logs service="sybil-api":
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} logs -f --tail 100 {{service}}'

# SSH into server
deploy-shell:
    ssh {{SERVER}}

# Arena bot status — text dashboard (readable by CLI / LLM)
arena-status hours="24":
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} exec -T sybil-arena-dashboard uv run python -m live.status --hours {{hours}}'

# Live system status (containers, blocks, traders, fills)
status:
    ./scripts/status.sh

# ── Contracts ───────────────────────────────────────────────────────────────

# Format Solidity contracts
contracts-fmt:
    cd contracts && forge fmt

# Check Solidity formatting
contracts-fmt-check:
    cd contracts && forge fmt --check

# Build Solidity contracts
contracts-build:
    cd contracts && forge build

# Run Solidity tests
contracts-test:
    cd contracts && forge test
