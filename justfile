# Justfile for matching simulation workspace

# Default recipe - show available commands
default:
    @just --list

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    RUSTFLAGS="-Dwarnings" cargo clippy --workspace --all-features

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

# Generate OpenVM app proving keys for the Sybil guest
openvm-keygen-app:
    cargo openvm keygen --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --app-only

# Generate OpenVM app and aggregation prefix proving keys for the Sybil guest
openvm-keygen:
    cargo openvm keygen --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil

# Generate or download OpenVM recursive proving and EVM verifier artifacts
openvm-setup:
    cargo openvm setup

openvm-setup-evm-download:
    cargo openvm setup --evm --download

# Print the app executable and VM commitments used by the on-chain verifier adapter
openvm-commit:
    cargo openvm commit --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil

# Convert a prepared guest input artifact into OpenVM CLI input JSON
openvm-input guest_input="/tmp/sybil-guest-input.msgpack" openvm_input="/tmp/sybil-openvm-input.json":
    cargo run --manifest-path zk/openvm-tools/Cargo.toml -- encode-input --guest-input {{guest_input}} --openvm-input {{openvm_input}}

# Run the Sybil OpenVM guest against an OpenVM CLI input JSON file
openvm-run input="/tmp/sybil-openvm-input.json":
    cargo openvm run --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input {{input}}

# Run local sequencer -> witgen -> prover input -> OpenVM smoke; prove=true adds app proof verification, never EVM proving.
zk-smoke prove="false":
    #!/usr/bin/env bash
    set -euo pipefail

    case "{{prove}}" in
      true|false) ;;
      *) echo "prove must be true or false" >&2; exit 2 ;;
    esac

    root="${SYBIL_ZK_SMOKE_DIR:-/tmp}"
    workdir="$root/sybil-zk-smoke-$(date +%s)-$$"
    store="$workdir/smoke.redb"
    job="$workdir/proof-job.msgpack"
    guest_input="$workdir/guest-input.msgpack"
    public_hash="$workdir/public-input-hash.hex"
    da_payload_dir="$workdir/da"
    da_manifest="$workdir/da-manifest.json"
    openvm_input="$workdir/openvm-input.json"
    app_proof="$workdir/openvm.app.proof"

    mkdir -p "$workdir"
    echo "zk_smoke_dir=$workdir"

    cargo run -p sybil-witgen-cli -- smoke-job --store "$store" --job "$job"
    cargo run -p sybil-prover -- prepare-file-da --job "$job" --guest-input "$guest_input" --payload-dir "$da_payload_dir" --manifest "$da_manifest" --public-input-hash "$public_hash"
    cargo run --manifest-path zk/openvm-tools/Cargo.toml -- encode-input --guest-input "$guest_input" --openvm-input "$openvm_input"
    CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" cargo openvm build --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil
    cargo openvm run --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input "$openvm_input"

    if [[ "{{prove}}" == "true" ]]; then
      CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" cargo openvm keygen --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --app-only
      cargo openvm prove app --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input "$openvm_input" --proof "$app_proof"
      cargo openvm verify app --manifest-path zk/openvm-guest/Cargo.toml --proof "$app_proof"
      echo "app_proof=$app_proof"
    fi

    echo "public_input_hash=$(cat "$public_hash")"
    echo "da_payload_dir=$da_payload_dir"
    echo "da_manifest=$da_manifest"
    echo "openvm_input=$openvm_input"
    echo "zk_smoke=ok"

# Generate an OpenVM app proof for the Sybil guest
openvm-prove-app input="/tmp/sybil-openvm-input.json" proof="/tmp/sybil-openvm.app.proof":
    cargo openvm prove app --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input {{input}} --proof {{proof}}

# Generate an OpenVM EVM proof for the Sybil guest
openvm-prove-evm input="/tmp/sybil-openvm-input.json" proof="/tmp/sybil-openvm.evm.proof":
    cargo openvm prove evm --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input {{input}} --proof {{proof}}

# Verify an OpenVM app proof for the Sybil guest
openvm-verify-app proof="/tmp/sybil-openvm.app.proof":
    cargo openvm verify app --manifest-path zk/openvm-guest/Cargo.toml --proof {{proof}}

# Verify an OpenVM EVM proof locally with the generated Halo2 verifier artifacts
openvm-verify-evm proof="/tmp/sybil-openvm.evm.proof":
    cargo openvm verify evm --proof {{proof}}

# Inspect a serialized state-transition proof job
prover-inspect job:
    cargo run -p sybil-prover -- inspect --job {{job}}

# Validate a proof job and emit a serialized OpenVM guest input artifact
prover-prepare job guest_input="/tmp/sybil-guest-input.msgpack" public_input_hash="/tmp/sybil-public-input-hash.hex":
    cargo run -p sybil-prover -- prepare --job {{job}} --guest-input {{guest_input}} --public-input-hash {{public_input_hash}}

# Validate a proof job, bind a deterministic file DA provider ref, and emit all host artifacts
prover-prepare-file-da job guest_input="/tmp/sybil-guest-input.msgpack" payload_dir="/tmp/sybil-da" manifest="/tmp/sybil-da-manifest.json" public_input_hash="/tmp/sybil-public-input-hash.hex":
    cargo run -p sybil-prover -- prepare-file-da --job {{job}} --guest-input {{guest_input}} --payload-dir {{payload_dir}} --manifest {{manifest}} --public-input-hash {{public_input_hash}}

# Run one local prover-worker scan over exported proof jobs
prover-worker-once jobs_dir="/tmp/sybil-prover-jobs" artifacts_dir="/tmp/sybil-prover-artifacts":
    cargo run -p sybil-prover -- worker --jobs-dir {{jobs_dir}} --artifacts-dir {{artifacts_dir}} --once

# Run the local prover worker continuously
prover-worker jobs_dir="/tmp/sybil-prover-jobs" artifacts_dir="/tmp/sybil-prover-artifacts" poll_ms="1000":
    cargo run -p sybil-prover -- worker --jobs-dir {{jobs_dir}} --artifacts-dir {{artifacts_dir}} --poll-ms {{poll_ms}}

# Serve prepared prover artifact status and Prometheus metrics
prover-serve artifacts_dir="/tmp/sybil-prover-artifacts" jobs_dir="/tmp/sybil-prover-jobs" bind="127.0.0.1:3002":
    cargo run -p sybil-prover -- serve --artifacts-dir {{artifacts_dir}} --jobs-dir {{jobs_dir}} --bind {{bind}}

# Write the canonical witness payload and provider-neutral DA manifest from prepared guest input
prover-publish-da guest_input="/tmp/sybil-guest-input.msgpack" payload="/tmp/sybil-da-witness.bin" manifest="/tmp/sybil-da-manifest.json":
    cargo run -p sybil-prover -- publish-da --guest-input {{guest_input}} --payload {{payload}} --manifest {{manifest}}

# Encode a SybilSettlement.submitStateRoot transaction from guest input and proof bytes
prover-submit-state-root settlement guest_input="/tmp/sybil-guest-input.msgpack" proof="/tmp/sybil-openvm.app.proof" calldata="/tmp/sybil-submit-state-root.calldata":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --calldata {{calldata}}

# Encode calldata plus a file-based eth_sendTransaction request for large proof bytes
prover-submit-state-root-rpc settlement from gas="0x1c9c380" guest_input="/tmp/sybil-guest-input.msgpack" proof="/tmp/sybil-openvm.app.proof" calldata="/tmp/sybil-submit-state-root.calldata" rpc_request="/tmp/sybil-submit-state-root-rpc.json":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --calldata {{calldata}} --rpc-request {{rpc_request}} --from {{from}} --gas {{gas}}

# Encode a real OpenVM EVM proof JSON for OpenVmVerifierAdapter and submitStateRoot
prover-submit-state-root-evm-rpc settlement from gas="0x1c9c380" guest_input="/tmp/sybil-guest-input.msgpack" proof="/tmp/sybil-openvm.evm.proof" calldata="/tmp/sybil-submit-state-root.calldata" rpc_request="/tmp/sybil-submit-state-root-rpc.json":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --proof-format openvm-evm-json --calldata {{calldata}} --rpc-request {{rpc_request}} --from {{from}} --gas {{gas}}

# Export the latest committed sequencer block as a portable proof job
witgen-export-latest store job="/tmp/sybil-proof-job.msgpack":
    cargo run -p sybil-witgen-cli -- export-latest --store {{store}} --job {{job}}

# Create a one-block local sequencer smoke fixture and export its proof job
witgen-smoke-job store="/tmp/sybil-smoke.redb" job="/tmp/sybil-proof-job.msgpack":
    cargo run -p sybil-witgen-cli -- smoke-job --store {{store}} --job {{job}}

# Clean and rebuild
rebuild:
    cargo clean && cargo build --release

# Build documentation
doc:
    cargo doc --workspace --no-deps

# Open documentation in browser
doc-open:
    cargo doc --workspace --no-deps --open

# Check all (compile, test, lint, fmt, docs)
check-all: fmt-check lint test docs-check contracts-fmt-check contracts-build contracts-test
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

# Smoke-test compose profile boundaries without starting containers
compose-smoke:
    ./scripts/compose-profile-smoke.sh

LOCAL_COMPOSE := "docker-compose"
DEPLOY_PLATFORM := "linux/amd64"

# Build Docker image
docker-build:
    {{LOCAL_COMPOSE}} build

# Start all services (API + VictoriaMetrics + Grafana)
docker-up:
    {{LOCAL_COMPOSE}} up -d

# Stop all services
docker-down:
    {{LOCAL_COMPOSE}} down

# Tail API logs
docker-logs:
    {{LOCAL_COMPOSE}} logs -f sybil-api

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
COMPOSE_TELEGRAM := "docker compose -f docker-compose.yml -f docker-compose.prod.yml -f docker-compose.telegram.yml"

# Sync compose configs + deploy/ directory to server
deploy-sync:
    ssh {{SERVER}} 'mkdir -p /opt/sybil/scripts && touch /opt/sybil/arena.env'
    scp docker-compose.yml docker-compose.prod.yml docker-compose.telegram.yml {{SERVER}}:/opt/sybil/
    scp -r deploy {{SERVER}}:/opt/sybil/
    scp scripts/ops-smoke.sh {{SERVER}}:/opt/sybil/scripts/

deploy-prod-env-check:
    ssh {{SERVER}} 'cd /opt/sybil && test -f .env && grep -q "^GF_SECURITY_ADMIN_PASSWORD=." .env && grep -q "^CADDY_OPS_AUTH_USER=." .env && grep -q "^CADDY_OPS_AUTH_HASH=." .env && grep -q "^SYBIL_SERVICE_TOKEN=." .env'

deploy-openrouter-env-check:
    ssh {{SERVER}} 'cd /opt/sybil && test -f arena.env && grep -q "^OPENROUTER_API_KEY=." arena.env'

# Build and deploy sybil-api, polymarket mirror, and prover status/mock API.
# The real filesystem prover worker is profile-gated until proof-job export is live.
deploy-api: deploy-sync deploy-prod-env-check
    DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build sybil-api
    docker save sybil-api:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d sybil-api sybil-polymarket sybil-prover sybil-prover-mock'

# Start the real filesystem prover worker when proof-job export is enabled.
deploy-prover-worker: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && COMPOSE_PROFILES=prover-worker {{COMPOSE_PROD}} up -d sybil-prover-worker'

# Destructively reset production app state, then restart services.
# This removes old markets, mirror mappings, arena bot DB, prover artifacts,
# and metric history from previous deploys. Pass CONFIRM explicitly.
deploy-reset-state confirm: deploy-prod-env-check
    @test "{{confirm}}" = "CONFIRM" || (echo 'Refusing to reset production state. Run: just deploy-reset-state CONFIRM' >&2; exit 2)
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} down; else {{COMPOSE_PROD}} down; fi'
    ssh {{SERVER}} 'docker volume rm sybil-data polymarket-data arena-data prover-jobs prover-artifacts sybil_prover-jobs sybil_prover-artifacts vmdata || true'
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} up -d --remove-orphans; else {{COMPOSE_PROD}} up -d --remove-orphans; fi'

# Build and deploy arena bots + dashboard. Requires OPENROUTER_API_KEY in /opt/sybil/arena.env.
deploy-arena: deploy-sync deploy-prod-env-check deploy-openrouter-env-check
    DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build sybil-arena
    docker save sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d sybil-arena sybil-arena-dashboard caddy'

# Deploy observability stack (node-exporter + VictoriaMetrics + vmalert + Grafana)
deploy-monitoring: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} up -d --remove-orphans node-exporter victoriametrics vmalert grafana telegram-alerts; else {{COMPOSE_PROD}} up -d --remove-orphans node-exporter victoriametrics vmalert grafana; fi'

# Enable Telegram delivery for vmalert alerts. Requires TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID in /opt/sybil/.env on the server.
deploy-telegram-alerts: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env && {{COMPOSE_TELEGRAM}} up -d telegram-alerts vmalert'

# Deploy Caddy HTTPS reverse proxy
deploy-caddy: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d caddy'

# Deploy everything
deploy-all: deploy-sync deploy-prod-env-check deploy-openrouter-env-check
    DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build
    docker save sybil-api:latest sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} up -d --remove-orphans; else {{COMPOSE_PROD}} up -d --remove-orphans; fi'

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

# Run a local Anvil bridge smoke with the explicit unsafe accept-all verifier.
# Start anvil separately, or point rpc_url at an existing Anvil-compatible RPC.
contracts-anvil-unsafe-smoke rpc_url="http://127.0.0.1:8545" private_key="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80":
    cd contracts && PRIVATE_KEY={{private_key}} forge script script/UnsafeAnvilSmoke.s.sol:UnsafeAnvilSmoke --rpc-url {{rpc_url}} --broadcast
