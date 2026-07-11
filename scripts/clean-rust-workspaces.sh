#!/usr/bin/env bash
# Remove target artifacts from the root and every standalone Cargo workspace.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"

manifests=(
    Cargo.toml
    fuzz/Cargo.toml
    zk/openvm-guest/Cargo.toml
    zk/openvm-escape-guest/Cargo.toml
    zk/openvm-tools/Cargo.toml
)

for manifest in "${manifests[@]}"; do
    echo "cleaning Cargo workspace: $manifest"
    cargo clean --manifest-path "$manifest"
done
