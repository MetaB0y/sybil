#!/usr/bin/env bash
# Compile every Cargo workspace intentionally excluded from the root workspace.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"

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
