#!/usr/bin/env bash
# Compile every Cargo workspace intentionally excluded from the root workspace.
#
# OpenVM guests are special: `openvm::init!` includes a config-derived
# `openvm_init.rs` that `cargo openvm build` generates and git deliberately
# ignores. Generate that host-check include first so this gate is reproducible
# from a clean clone. Real guest-target/commitment rebuilding remains the
# separate `just zk-rebuild-check` validity gate.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"

./scripts/generate-openvm-init.py \
    zk/openvm-guest \
    zk/openvm-escape-guest

manifests=(
    fuzz/Cargo.toml
    zk/openvm-guest/Cargo.toml
    zk/openvm-escape-guest/Cargo.toml
    zk/openvm-tools/Cargo.toml
)

for manifest in "${manifests[@]}"; do
    echo "checking standalone workspace: $manifest"
    cargo check --locked --all-targets --manifest-path "$manifest"
done
