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

SERVER := "root@172.104.31.54"

# Build and deploy sybil-api + polymarket mirror to server
deploy-api:
    docker build -t sybil-api:latest .
    docker save sybil-api:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'docker stop sybil-api sybil-polymarket 2>/dev/null; docker rm sybil-api sybil-polymarket 2>/dev/null; true'
    ssh {{SERVER}} 'docker run --rm -v polymarket-data:/data alpine rm -f /data/polymarket_mapping.json'
    ssh {{SERVER}} 'docker run -d --name sybil-api --restart unless-stopped \
        -p 3000:3000 \
        -e SYBIL_DEV_MODE=true -e SYBIL_BLOCK_INTERVAL_MS=2000 -e RUST_LOG=info \
        sybil-api:latest'
    ssh {{SERVER}} 'docker run -d --name sybil-polymarket --restart unless-stopped \
        -v polymarket-data:/data -e RUST_LOG=sybil_polymarket=info \
        --entrypoint sybil-polymarket sybil-api:latest \
        --sybil-url http://172.17.0.1:3000 --max-events 50 --mm-half-spread 0.01 \
        --mm-budget-dollars 5000 --mm-initial-balance-dollars 1000000 \
        --mapping-store-path /data/polymarket_mapping.json --sync-interval-secs 120'

# Build and deploy arena bots (pass OpenRouter key)
deploy-arena key:
    cd arena && docker build -t sybil-arena:latest .
    docker save sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'docker stop sybil-arena 2>/dev/null; docker rm sybil-arena 2>/dev/null; true'
    ssh {{SERVER}} 'docker run -d --name sybil-arena --restart unless-stopped \
        -v arena-data:/data -v polymarket-data:/polymarket-data:ro -e PYTHONUNBUFFERED=1 \
        sybil-arena:latest \
        --sybil-url http://172.17.0.1:3000 --api-key {{key}} \
        --max-markets 20 --model minimax/minimax-m2.7 --db-path /data/decisions.db \
        --mapping-path /polymarket-data/polymarket_mapping.json'

# Deploy arena dashboard (arena image must be loaded already)
deploy-dashboard:
    ssh {{SERVER}} 'docker stop sybil-arena-dashboard 2>/dev/null; docker rm sybil-arena-dashboard 2>/dev/null; true'
    ssh {{SERVER}} 'docker run -d --name sybil-arena-dashboard --restart unless-stopped \
        -v arena-data:/data -p 8501:8501 -e PYTHONUNBUFFERED=1 \
        --entrypoint uv sybil-arena:latest \
        run streamlit run live/dashboard.py \
        --server.port=8501 --server.address=0.0.0.0 --server.headless=true'

# Deploy everything (api + polymarket + arena + dashboard)
deploy-all key:
    just deploy-api
    just deploy-arena {{key}}
    just deploy-dashboard

# Tail logs from a container on the server
deploy-logs service="sybil-api":
    ssh {{SERVER}} docker logs -f --tail 100 {{service}}

# SSH into server
deploy-shell:
    ssh {{SERVER}}

# Arena bot status — text dashboard (readable by CLI / LLM)
arena-status hours="24":
    ssh {{SERVER}} 'docker exec sybil-arena-dashboard uv run python -m live.status --hours {{hours}}'

# Live system status (containers, blocks, traders, fills)
status:
    #!/usr/bin/env bash
    set -e
    S="root@172.104.31.54"
    echo "=== Containers ==="
    ssh $S 'docker ps --format "table {{{{.Names}}}}\t{{{{.Status}}}}"'
    echo ""
    echo "=== Recent Blocks ==="
    ssh $S 'timeout 8 curl -sN http://localhost:3000/v1/blocks/stream 2>/dev/null' | head -4 | while read -r line; do
        echo "$line" | python3 -c "
import sys,json
for l in sys.stdin:
    l=l.strip()
    if l.startswith('data: '):
        b=json.loads(l[6:])
        f='<<<' if b['fill_count']>0 else ''
        print(f'  Block {b[\"height\"]}: {b[\"order_count\"]} orders, {b[\"fill_count\"]} fills, {len(b[\"rejections\"])} rej, welfare=\${b[\"total_welfare_nanos\"]/1e9:.2f} {f}')
" 2>/dev/null
    done
    echo ""
    echo "=== Trader Activity ==="
    LOGS=$(ssh $S 'docker logs sybil-arena 2>&1' | grep -v httpx)
    echo "  LLM calls:     $(echo "$LOGS" | grep -c 'LLM response' || echo 0)"
    echo "  Parse failures: $(echo "$LOGS" | grep -c 'Failed to parse' || echo 0)"
    echo "  Trade orders:   $(echo "$LOGS" | grep -c 'Buy' || echo 0)"
    echo "  HOLDs:          $(echo "$LOGS" | grep -c 'HOLD' || echo 0)"
    echo ""
    echo "=== Balances ==="
    for id in 11 12 13; do
        ssh $S "curl -s http://localhost:3000/v1/accounts/$id" 2>/dev/null | python3 -c "
import sys,json
try:
    a=json.load(sys.stdin); print(f'  Account {a[\"id\"]}: \${a[\"balance_nanos\"]/1e9:.2f}')
except: pass
"
    done
    echo ""
    echo "=== News Pipeline ==="
    echo "  Polls: $(echo "$LOGS" | grep -c 'Poll:' || echo 0)"
    echo "  Articles gated: $(echo "$LOGS" | grep -c '✓' || echo 0)"
    echo ""
    echo "=== Recent Decisions ==="
    echo "$LOGS" | grep 'FV=' | tail -10
