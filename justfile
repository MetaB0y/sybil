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

# Pre-commit check (fmt + clippy, ~3s with warm cache)
pre-commit:
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
    ssh {{SERVER}} 'cd /opt/sybil && OPENROUTER_API_KEY={{key}} {{COMPOSE_PROD}} up -d sybil-arena sybil-arena-dashboard'

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
    ssh {{SERVER}} docker logs -f --tail 100 {{service}}

# SSH into server
deploy-shell:
    ssh {{SERVER}}

# Arena bot status — text dashboard (readable by CLI / LLM)
arena-status hours="24":
    ssh {{SERVER}} 'docker exec sybil-arena-dashboard uv run python -m live.status --hours {{hours}}'

# ── Composition Demo ───────────────────────────────────────────────────────

# Start local composition-demo agent gateway (requires sybil-api separately)
composition-demo-agent sybil_url="http://localhost:3001":
    cd arena && uv run python -m live.composition_demo.server --sybil-url {{sybil_url}}

# Import Polymarket/Kalshi source metadata into the local composition registry
composition-demo-import:
    cd arena && uv run python -m live.composition_demo.import_sources --max-atoms 300

# Seed imported atom/composition markets into a dev-mode sybil-api
composition-demo-seed sybil_url="http://localhost:3001":
    cd arena && uv run python -m live.composition_demo.seed --sybil-url {{sybil_url}}

# Run the reference MM quote loop for the composition demo
composition-demo-mm sybil_url="http://localhost:3001":
    cd arena && uv run python -m live.composition_demo.mm_loop --sybil-url {{sybil_url}}

# Start the custom React/Vite composition demo UI
composition-demo-ui:
    cd apps/composition-demo && npm run dev

# Run the full local composition demo stack: API + agent gateway + seeding + MM + UI
composition-demo port="3001":
    #!/usr/bin/env bash
    set -euo pipefail
    sybil_url="http://localhost:{{port}}"
    cleanup() {
      jobs -pr | xargs -r kill
    }
    trap cleanup EXIT INT TERM

    rm -f arena/live/composition_demo/state.json

    cargo run -p sybil-api --bin sybil-api -- --dev-mode --port {{port}} &
    api_pid=$!

    for _ in $(seq 1 60); do
      if curl -fsS "$sybil_url/v1/health" >/dev/null 2>&1; then
        break
      fi
      sleep 0.5
    done

    cd arena
    uv run python -m live.composition_demo.server --sybil-url "$sybil_url" &
    agent_pid=$!
    uv run python -m live.composition_demo.import_sources --max-atoms 300
    uv run python -m live.composition_demo.seed --sybil-url "$sybil_url"
    uv run python -m live.composition_demo.mm_loop --sybil-url "$sybil_url" &
    mm_pid=$!
    cd ../apps/composition-demo
    npm run dev

# Live system status (containers, blocks, traders, fills)
status:
    ./scripts/status.sh
