# Justfile for matching simulation workspace

# Default recipe - show available commands
default:
    @just --list

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    RUSTFLAGS="-Dwarnings" cargo clippy --workspace --all-targets --all-features

# Compile every root-workspace target and feature combination.
workspace-check:
    cargo check --workspace --all-targets --all-features

# Keep the compiler, Edition, manifests, workflows, and Docker image aligned.
rust-workspaces-check:
    ./scripts/check-rust-workspaces.py

# Compile every Cargo workspace intentionally excluded from the root workspace.
standalone-check:
    ./scripts/check-rust-standalone.sh

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Regenerate the single Rust/Solidity canonical golden-vector artifact.
golden-write:
    cargo run -p sybil-golden-vectors --bin emit-golden -- --write

# Fail if canonical encoders no longer reproduce the committed artifact.
golden-check:
    cargo run -p sybil-golden-vectors --bin emit-golden -- --check

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

# Agent-based multi-batch sequencer simulation (sequencer-sim crate)
sim-agent scenario="standard":
    cargo run --bin sybil-sim --release -- --scenario {{scenario}} -v

# Build release
build:
    cargo build --release

# Install the OpenVM 2.0 CLI used by the ZK guest tooling
openvm-install:
    cargo +1.91 install --locked --git https://github.com/openvm-org/openvm.git --tag v2.0.0 cargo-openvm

# Host-check the OpenVM guest from a clean checkout. The generated init include
# is only for host compilation; the real guest target remains openvm-guest-build.
openvm-guest-check:
    ./scripts/generate-openvm-init.py zk/openvm-guest
    cargo check --locked --all-targets --manifest-path zk/openvm-guest/Cargo.toml

# Host-check the independent escape guest from a clean checkout.
openvm-escape-guest-check:
    ./scripts/generate-openvm-init.py zk/openvm-escape-guest
    cargo check --locked --all-targets --manifest-path zk/openvm-escape-guest/Cargo.toml

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

# Print the app executable and VM commitments used by the on-chain verifier adapter.
# OpenVM v2.0.0 forwards RUSTFLAGS to the guest rustc invocation; remap all
# checkout/toolchain roots that can otherwise leak into panic and debug strings.
openvm-commit output_dir="target/openvm/sybil":
    ./scripts/openvm-commit.sh main {{output_dir}}

# Build and print the independent Form-L escape guest commitments. This is a
# commitment-only operation: it does not run setup, keygen, or proving.
openvm-escape-commit output_dir="target/openvm/sybil-escape":
    ./scripts/openvm-commit.sh escape {{output_dir}}

openvm-commit-all:
    just openvm-commit target/openvm/sybil
    just openvm-escape-commit target/openvm/sybil-escape

# Local from-source guest rebuild gate (SYB-233, CI-off era): regenerate the
# guest commitment into a scratch dir, require byte-equality with the committed
# commit.json, then run the fingerprint staleness check. Run before landing any
# guest-closure change (and after a repin) while Actions billing is off — it is
# the local stand-in for the zk-rebuild CI hard gate.
zk-rebuild-check:
    #!/usr/bin/env bash
    set -euo pipefail
    export TMPDIR="${TMPDIR:-/home/anonymous/.cache/tmp}"
    check_guest() {
        local label="$1" recipe="$2" release_dir="$3" package="$4" output_dir="$5"
        local committed="$release_dir/$package.commit.json"
        local baseline="$release_dir/$package.baseline.json"
        local snapshot baseline_snapshot regenerated field want got
        snapshot="$(mktemp "$TMPDIR/zk-rebuild-check-commit.XXXXXX.json")"
        baseline_snapshot="$(mktemp "$TMPDIR/zk-rebuild-check-baseline.XXXXXX.json")"
        cp "$committed" "$snapshot"
        cp "$baseline" "$baseline_snapshot"
        just "$recipe" "$output_dir"
        cp "$snapshot" "$committed"
        cp "$baseline_snapshot" "$baseline"
        regenerated="$output_dir/$package.commit.json"
        for field in app_exe_commit app_vm_commit; do
            want="$(jq -r ".$field" "$snapshot")"
            got="$(jq -r ".$field" "$regenerated")"
            if [[ "$want" != "$got" ]]; then
                echo "FAIL: $label $field mismatch — committed $want, regenerated $got" >&2
                echo "      (source changed without a repin, or the build stopped reproducing)" >&2
                exit 1
            fi
        done
        rm -f "$snapshot" "$baseline_snapshot"
        echo "OK: $label from-source rebuild reproduces committed commitments"
    }
    check_guest \
      "main guest" \
      "openvm-commit" \
      "zk/openvm-guest/openvm/release" \
      "sybil-openvm-guest" \
      "target/openvm/zk-rebuild-check-main"
    check_guest \
      "escape guest" \
      "openvm-escape-commit" \
      "zk/openvm-escape-guest/openvm/release" \
      "sybil-openvm-escape-guest" \
      "target/openvm/zk-rebuild-check-escape"
    scripts/zk-guest-fingerprint.sh --check
    echo "OK: both from-source rebuilds reproduce committed commitments; fingerprints fresh"

# Build a persisted keyed/traded state, export selective qMDB proofs, and run
# the Form-L guest. No setup, keygen, or proving is performed.
zk-escape-smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    export TMPDIR="${TMPDIR:-/home/anonymous/.cache/tmp}"
    workdir="$TMPDIR/sybil-escape-smoke-$PPID-$$"
    store="$workdir/state.redb"
    guest_input="$workdir/guest-input.msgpack"
    openvm_input="$workdir/openvm-input.json"
    mkdir -p "$workdir"
    cargo run -p sybil-prover --features sequencer-store -- witgen escape-smoke \
      --store "$store" --guest-input "$guest_input"
    cargo run --manifest-path zk/openvm-tools/Cargo.toml -- encode-escape-input \
      --guest-input "$guest_input" --openvm-input "$openvm_input"
    CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" cargo openvm run \
      --manifest-path zk/openvm-escape-guest/Cargo.toml \
      --config zk/openvm-escape-guest/openvm.toml \
      --output-dir target/openvm/sybil-escape \
      --input "$openvm_input"
    echo "escape_smoke_dir=$workdir"
    echo "zk_escape_smoke=ok"

# Convert a prepared guest input artifact into OpenVM CLI input JSON
openvm-input guest_input="target/sybil-guest-input.msgpack" openvm_input="target/sybil-openvm-input.json":
    cargo run --manifest-path zk/openvm-tools/Cargo.toml -- encode-input --guest-input {{guest_input}} --openvm-input {{openvm_input}}

# Run the Sybil OpenVM guest against an OpenVM CLI input JSON file
openvm-run input="target/sybil-openvm-input.json":
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

    cargo run -p sybil-prover --features sequencer-store -- witgen smoke-job --store "$store" --job "$job"
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
openvm-prove-app input="target/sybil-openvm-input.json" proof="target/sybil-openvm.app.proof":
    cargo openvm prove app --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input {{input}} --proof {{proof}}

# Generate an OpenVM EVM proof for the Sybil guest
openvm-prove-evm input="target/sybil-openvm-input.json" proof="target/sybil-openvm.evm.proof":
    cargo openvm prove evm --manifest-path zk/openvm-guest/Cargo.toml --config zk/openvm-guest/openvm.toml --output-dir target/openvm/sybil --input {{input}} --proof {{proof}}

# Verify an OpenVM app proof for the Sybil guest
openvm-verify-app proof="target/sybil-openvm.app.proof":
    cargo openvm verify app --manifest-path zk/openvm-guest/Cargo.toml --proof {{proof}}

# Verify an OpenVM EVM proof locally with the generated Halo2 verifier artifacts
openvm-verify-evm proof="target/sybil-openvm.evm.proof":
    cargo openvm verify evm --proof {{proof}}

# Inspect a serialized state-transition proof job
prover-inspect job:
    cargo run -p sybil-prover -- inspect --job {{job}}

# Validate a proof job and emit a serialized OpenVM guest input artifact
prover-prepare job guest_input="target/sybil-guest-input.msgpack" public_input_hash="target/sybil-public-input-hash.hex":
    cargo run -p sybil-prover -- prepare --job {{job}} --guest-input {{guest_input}} --public-input-hash {{public_input_hash}}

# Validate a proof job, bind a deterministic file DA provider ref, and emit all host artifacts
prover-prepare-file-da job guest_input="target/sybil-guest-input.msgpack" payload_dir="target/sybil-da" manifest="target/sybil-da-manifest.json" public_input_hash="target/sybil-public-input-hash.hex":
    cargo run -p sybil-prover -- prepare-file-da --job {{job}} --guest-input {{guest_input}} --payload-dir {{payload_dir}} --manifest {{manifest}} --public-input-hash {{public_input_hash}}

# Run one local prover-worker scan over exported proof jobs
prover-worker-once jobs_dir="target/sybil-prover-jobs" artifacts_dir="target/sybil-prover-artifacts":
    cargo run -p sybil-prover -- worker --jobs-dir {{jobs_dir}} --artifacts-dir {{artifacts_dir}} --once

# Run the local prover worker continuously
prover-worker jobs_dir="target/sybil-prover-jobs" artifacts_dir="target/sybil-prover-artifacts" poll_ms="1000":
    cargo run -p sybil-prover -- worker --jobs-dir {{jobs_dir}} --artifacts-dir {{artifacts_dir}} --poll-ms {{poll_ms}}

# Serve prepared prover artifact status and Prometheus metrics
prover-serve artifacts_dir="target/sybil-prover-artifacts" jobs_dir="target/sybil-prover-jobs" bind="127.0.0.1:3002":
    cargo run -p sybil-prover -- serve --artifacts-dir {{artifacts_dir}} --jobs-dir {{jobs_dir}} --bind {{bind}}

# Write the canonical witness payload and provider-neutral DA manifest from prepared guest input
prover-publish-da guest_input="target/sybil-guest-input.msgpack" payload="target/sybil-da-witness.bin" manifest="target/sybil-da-manifest.json":
    cargo run -p sybil-prover -- publish-da --guest-input {{guest_input}} --payload {{payload}} --manifest {{manifest}}

# Encode a SybilSettlement.submitStateRoot transaction from guest input and proof bytes
prover-submit-state-root settlement guest_input="target/sybil-guest-input.msgpack" proof="target/sybil-openvm.app.proof" calldata="target/sybil-submit-state-root.calldata":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --calldata {{calldata}}

# Encode calldata plus a file-based eth_sendTransaction request for large proof bytes
prover-submit-state-root-rpc settlement from gas="0x1c9c380" guest_input="target/sybil-guest-input.msgpack" proof="target/sybil-openvm.app.proof" calldata="target/sybil-submit-state-root.calldata" rpc_request="target/sybil-submit-state-root-rpc.json":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --calldata {{calldata}} --rpc-request {{rpc_request}} --from {{from}} --gas {{gas}}

# Encode a real OpenVM EVM proof JSON for OpenVmVerifierAdapter and submitStateRoot
prover-submit-state-root-evm-rpc settlement from gas="0x1c9c380" guest_input="target/sybil-guest-input.msgpack" proof="target/sybil-openvm.evm.proof" calldata="target/sybil-submit-state-root.calldata" rpc_request="target/sybil-submit-state-root-rpc.json":
    cargo run -p sybil-prover -- submit-state-root --settlement {{settlement}} --guest-input {{guest_input}} --proof {{proof}} --proof-format openvm-evm-json --calldata {{calldata}} --rpc-request {{rpc_request}} --from {{from}} --gas {{gas}}

# Export the latest committed sequencer block as a portable proof job
witgen-export-latest store job="target/sybil-proof-job.msgpack":
    cargo run -p sybil-prover --features sequencer-store -- witgen export-latest --store {{store}} --job {{job}}

# Create a one-block local sequencer smoke fixture and export its proof job
witgen-smoke-job store="target/sybil-smoke.redb" job="target/sybil-proof-job.msgpack":
    cargo run -p sybil-prover --features sequencer-store -- witgen smoke-job --store {{store}} --job {{job}}

# Clean and rebuild
rebuild:
    cargo clean && cargo build --release

# Build documentation
doc:
    cargo doc --workspace --no-deps

# Open documentation in browser
doc-open:
    cargo doc --workspace --no-deps --open

# Run Arena Python lint and tests
arena-check:
    cd arena && uv run ruff check .
    cd arena && uv run pytest -q

# Regenerate the vendored Sybil OpenAPI Python client (arena/sybil_client/_generated)
# from the live spec. Mirrors the frontend's `types:generate`; boots sybil-api on a
# free port, fetches /openapi.json, and regenerates only the _generated/ package.
arena-sdk-regen:
    ./arena/scripts/regen-sdk.sh

# Run frontend typecheck, lint, vitest, and build (mirrors .github/workflows/frontend.yml).
# Degrades gracefully with a clear skip message when pnpm is not installed locally.
frontend-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v pnpm >/dev/null 2>&1; then
        echo "SKIP frontend-check: pnpm not found (install pnpm@10 to run frontend/web checks)"
        exit 0
    fi
    cd frontend/web
    pnpm install --frozen-lockfile
    pnpm tsc --noEmit
    pnpm lint
    pnpm test
    pnpm build

# Fast developer gate: metadata, formatting, compilation, and lints.
check-fast: rust-workspaces-check fmt-check workspace-check lint

# Consensus/protocol gate: shared vectors, guest inputs, deployment coordination,
# an explicit validity deployment boundary, and generated protocol documentation
# must all agree.
check-consensus: golden-check
    ./scripts/zk-guest-fingerprint.sh --check
    ./scripts/update-validity-pins.py --check
    ./scripts/test-validity-boundary.py
    ./scripts/check-validity-boundary.py --check
    ./scripts/update-protocol-pins.py --check

# Backup/restore manifest compatibility and shell-entrypoint syntax.
store-tools-check:
    python3 scripts/test-store-manifest.py
    python3 -m py_compile scripts/store-manifest.py scripts/test-store-manifest.py
    bash -n scripts/store-backup.sh scripts/store-restore-drill.sh

# Complete local/CI-equivalent gate, including every standalone Rust workspace.
check-all: check-fast test standalone-check check-consensus docs-check store-tools-check arena-check frontend-check monitoring-check contracts-fmt-check contracts-build contracts-test
    @echo "All checks passed!"

# Run benchmarks if any
bench:
    cargo bench --workspace

# Watch for changes and run tests
watch:
    cargo watch -x "test --workspace"

# Clean build artifacts from the root and all standalone Cargo workspaces.
clean:
    ./scripts/clean-rust-workspaces.sh

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

# Validate vault metadata/paths and build the full documentation site strictly.
docs-check:
    ./scripts/update-protocol-pins.py --check
    ./scripts/check-doc-sync.py
    ./scripts/check-vault.sh
    NO_MKDOCS_2_WARNING=1 PYTHONWARNINGS=ignore::DeprecationWarning uvx --with mkdocs==1.6.1 --with mkdocs-material==9.7.6 --with mkdocs-roamlinks-plugin==0.3.2 mkdocs build --strict

# Render every maintained Mermaid diagram with the pinned official CLI image.
docs-mermaid:
    ./scripts/check-mermaid.sh

# Regenerate/check the compact page sourced from protocol constants and artifacts.
docs-pins-write:
    ./scripts/update-protocol-pins.py --write

docs-pins-check:
    ./scripts/update-protocol-pins.py --check

# Refresh/check the desired guest pins and separately recorded deployment state.
validity-pins-write:
    ./scripts/update-validity-pins.py --write-desired

validity-pins-check:
    ./scripts/update-validity-pins.py --check

# Bind the current validity artifacts to a reviewed fresh-genesis or migration
# decision. Environment variables avoid embedding review text in shell source.
validity-boundary-write:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${VALIDITY_BOUNDARY_ACTION:?set VALIDITY_BOUNDARY_ACTION}"
    : "${VALIDITY_BOUNDARY_REASON:?set VALIDITY_BOUNDARY_REASON}"
    args=(--write --action "$VALIDITY_BOUNDARY_ACTION" --reason "$VALIDITY_BOUNDARY_REASON")
    if [[ -n "${VALIDITY_BOUNDARY_REFERENCE:-}" ]]; then
        args+=(--reference "$VALIDITY_BOUNDARY_REFERENCE")
    fi
    ./scripts/check-validity-boundary.py "${args[@]}"

validity-boundary-check:
    ./scripts/test-validity-boundary.py
    ./scripts/check-validity-boundary.py --check

# Check workspace/design inventories against current repository structure.
docs-sync:
    ./scripts/check-doc-sync.py

# Check public links in maintained Markdown. Confirmed 404/410 responses fail;
# authentication, rate limits, and transient network errors are warnings.
docs-links:
    ./scripts/check-external-links.py

# List notes with last_verified > 90 days
docs-stale:
    @find docs/architecture -type f -name '*.md' -print | sort | while IFS= read -r f; do \
        lv="$(awk '/^---$/{n++; next} n==1 && /^last_verified:/{print $2; exit}' "$f")"; \
        [ -z "$lv" ] && continue; \
        days=$(( ($(date +%s) - $(date -d "$lv" +%s 2>/dev/null || echo $(date +%s))) / 86400 )); \
        [ "$days" -gt 90 ] && echo "  $days days: $(basename "$f" .md) (last: $lv)"; \
    done; true

# Search vault content
docs-search term:
    @grep -rni "{{term}}" docs/architecture --include='*.md' | sed 's|docs/architecture/||'

# List all notes with layer + status
docs-list:
    @find docs/architecture -type f -name '*.md' -print | sort | while IFS= read -r f; do \
        layer="$(awk '/^---$/{n++; next} n==1 && /^layer:/{print $2; exit}' "$f")"; \
        status="$(awk '/^---$/{n++; next} n==1 && /^status:/{print $2; exit}' "$f")"; \
        printf "  %-12s %-12s %s\n" "$layer" "$status" "${f#docs/architecture/}"; \
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

# Validate the monitoring scrape config, alert syntax, and focused rule semantics.
# Use a local promtool when installed; otherwise run the pinned tool image locally.
monitoring-check: compose-smoke
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v promtool >/dev/null 2>&1; then
        promtool check config deploy/prometheus.yml
        promtool check rules deploy/vmalert/rules.yml
        promtool test rules deploy/vmalert/tests/arena-liveness_test.yml
    elif command -v docker >/dev/null 2>&1; then
        image="prom/prometheus:v2.52.0"
        docker run --rm --entrypoint promtool -v "$PWD/deploy:/work:ro" "$image" check config /work/prometheus.yml
        docker run --rm --entrypoint promtool -v "$PWD/deploy/vmalert:/work:ro" "$image" check rules /work/rules.yml
        docker run --rm --entrypoint promtool -v "$PWD/deploy/vmalert:/work:ro" "$image" test rules /work/tests/arena-liveness_test.yml
    else
        echo "monitoring-check requires promtool or Docker" >&2
        exit 2
    fi

# Isolated Docker money-path E2E (SYB-243). Builds sybil-api, runs the shared
# deterministic signed seeder, asserts exact fills/prices/balances, then down -v.
itest-compose:
    ./scripts/itest-compose.sh

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

# Post-deploy verification gates (SYB-248) run against the LIVE stack as the
# final step of application deploy recipes. API/all-stack promotions run the
# full deterministic market/fill seed. Scoped web/Arena promotions run every
# other assertion but do not create another persistent fixture market.
#
# The deploy is orchestrated from this source checkout, where post-deploy smoke
# builds (or reuses) the canonical `smoke_sign` helper locally. Signed order and
# cancel are core private-devnet flows, so every deploy requires the signer and
# fails closed if it is unavailable or either lifecycle check regresses.

# Sync compose configs + deploy/ directory to server
deploy-sync:
    ssh {{SERVER}} 'mkdir -p /opt/sybil/scripts/lib && touch /opt/sybil/arena.env'
    scp docker-compose.yml docker-compose.prod.yml docker-compose.telegram.yml {{SERVER}}:/opt/sybil/
    scp -r deploy {{SERVER}}:/opt/sybil/
    scp scripts/ops-smoke.sh scripts/store-backup.sh scripts/store-restore-drill.sh scripts/store-manifest.py scripts/synthetic-probe.sh {{SERVER}}:/opt/sybil/scripts/
    scp scripts/lib/smoke-common.sh {{SERVER}}:/opt/sybil/scripts/lib/

deploy-prod-env-check:
    ssh {{SERVER}} 'cd /opt/sybil && test -f .env && grep -q "^GF_SECURITY_ADMIN_PASSWORD=." .env && grep -q "^CADDY_OPS_AUTH_USER=." .env && grep -q "^CADDY_OPS_AUTH_HASH=." .env && grep -q "^SYBIL_SERVICE_TOKEN=." .env && grep -q "^SYBIL_WEBAUTHN_RP_ID=." .env && grep -q "^SYBIL_WEBAUTHN_ORIGIN=." .env'

deploy-openrouter-env-check:
    ssh {{SERVER}} 'cd /opt/sybil && test -f arena.env && grep -q "^OPENROUTER_API_KEY=." arena.env'

# Build and deploy sybil-api, polymarket mirror, and prover status/mock API.
# The real filesystem prover worker is profile-gated until proof-job export is live.
deploy-api: deploy-sync deploy-prod-env-check && deploy-verify
    DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build sybil-api
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
deploy-arena: deploy-sync deploy-prod-env-check deploy-openrouter-env-check && deploy-verify-scoped
    DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build sybil-arena
    docker save sybil-arena:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d sybil-arena sybil-arena-dashboard caddy'

# Deploy observability stack (node-exporter + VictoriaMetrics + vmalert + Grafana)
deploy-monitoring: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} up -d --remove-orphans node-exporter victoriametrics vmalert grafana telegram-alerts; else {{COMPOSE_PROD}} up -d --remove-orphans node-exporter victoriametrics vmalert grafana; fi'

# Enable Telegram delivery for vmalert alerts. Requires TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID in /opt/sybil/.env on the server.
deploy-telegram-alerts: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env && {{COMPOSE_TELEGRAM}} up -d telegram-alerts vmalert'

# Build and deploy the Next.js web frontend, then reload Caddy for its vhost.
# NEXT_PUBLIC_* are baked at build time; override the API/WS base or WebAuthn
# rpId by exporting them before running, e.g.:
#   NEXT_PUBLIC_API_BASE=https://api.sybil.exchange \
#   NEXT_PUBLIC_WS_BASE=wss://api.sybil.exchange \
#   NEXT_PUBLIC_WEBAUTHN_RP_ID=sybil.exchange just deploy-web
deploy-web: deploy-sync deploy-prod-env-check && deploy-verify-scoped
    DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build sybil-web
    docker save sybil-web:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d sybil-web caddy'

# Deploy Caddy HTTPS reverse proxy
deploy-caddy: deploy-sync deploy-prod-env-check
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} up -d caddy'

# Deploy everything
deploy-all: deploy-sync deploy-prod-env-check deploy-openrouter-env-check && deploy-verify
    DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_DEFAULT_PLATFORM={{DEPLOY_PLATFORM}} {{LOCAL_COMPOSE}} build
    docker save sybil-api:latest sybil-arena:latest sybil-web:latest | ssh {{SERVER}} docker load
    ssh {{SERVER}} 'cd /opt/sybil && if test -f .env && grep -q "^TELEGRAM_BOT_TOKEN=." .env && grep -q "^TELEGRAM_CHAT_ID=." .env; then {{COMPOSE_TELEGRAM}} up -d --remove-orphans; else {{COMPOSE_PROD}} up -d --remove-orphans; fi'

# Post-deploy smoke GATE against the LIVE stack (SYB-248). Fail-closed: exits
# non-zero if any core flow is broken (health, CORS, passkey onboarding,
# deterministic fills-after-seed, service-token matrix), which fails the deploy.
# The service token is read from /opt/sybil/.env on the server; per-container
# health is probed over SSH (SYBIL_SMOKE_DOCKER_SSH={{SERVER}}). Runs
# automatically as the final step of deploy-api / deploy-all; can also be
# invoked directly.
deploy-verify:
    SYBIL_SMOKE_DOCKER_SSH={{SERVER}} scripts/post-deploy-smoke.sh --require-signer --service-token "$(ssh {{SERVER}} 'grep -oP "^SYBIL_SERVICE_TOKEN=\K.*" /opt/sybil/.env')"

# Scoped verifier for web/Arena image promotions. The API/matcher did not
# change, so retain every other fail-closed assertion while avoiding another
# durable SYB-247 market solely to re-prove the unchanged matcher.
deploy-verify-scoped:
    SYBIL_SMOKE_DOCKER_SSH={{SERVER}} scripts/post-deploy-smoke.sh --require-signer --skip-fill-seed --service-token "$(ssh {{SERVER}} 'grep -oP "^SYBIL_SERVICE_TOKEN=\K.*" /opt/sybil/.env')"

# Restart-resilience gate (SYB-267): restarts the live sybil-api container and
# fails on OOM-kill / boot-loop / unhealthy-after-timeout. OPT-IN — ~20s API
# downtime, so it is NOT part of the auto-run deploy-verify. Run before demos
# and after memory/config changes.
deploy-verify-restart:
    scripts/restart-resilience-check.sh --ssh {{SERVER}}

# Tail logs from a container on the server
deploy-logs service="sybil-api":
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} logs -f --tail 100 {{service}}'

# SSH into server
deploy-shell:
    ssh {{SERVER}}

# Arena bot status — text dashboard (readable by CLI / LLM)
arena-status hours="24":
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} exec -T sybil-arena-dashboard uv run --no-sync python -m live.status --hours {{hours}}'

# Preview resolved-market labels that would be added to the live decisions DB.
arena-outcomes-dry-run:
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} exec -T sybil-arena-dashboard uv run --no-sync python -m scripts.record_outcomes --db /data/decisions.db --api-base http://sybil-api:3000 --dry-run'

# Persist conflict-checked resolved-market labels used by calibration reports.
arena-record-outcomes:
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} exec -T sybil-arena-dashboard uv run --no-sync python -m scripts.record_outcomes --db /data/decisions.db --api-base http://sybil-api:3000'

# Print the live bot calibration/rejection report from the shared arena volume.
arena-calibration:
    ssh {{SERVER}} 'cd /opt/sybil && {{COMPOSE_PROD}} exec -T sybil-arena-dashboard uv run --no-sync python -m scripts.calibration --db /data/decisions.db'

# Live system status (containers, blocks, traders, fills)
status:
    ./scripts/status.sh

# ── Docs ────────────────────────────────────────────────────────────────────

# Serve the docs site locally with live reload (http://127.0.0.1:8000)
docs-serve:
    NO_MKDOCS_2_WARNING=1 PYTHONWARNINGS=ignore::DeprecationWarning uvx --with mkdocs==1.6.1 --with mkdocs-material==9.7.6 --with mkdocs-roamlinks-plugin==0.3.2 mkdocs serve

# Build the static docs site into ./site
docs-build:
    NO_MKDOCS_2_WARNING=1 PYTHONWARNINGS=ignore::DeprecationWarning uvx --with mkdocs==1.6.1 --with mkdocs-material==9.7.6 --with mkdocs-roamlinks-plugin==0.3.2 mkdocs build --strict

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
