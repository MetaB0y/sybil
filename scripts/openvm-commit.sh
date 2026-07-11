#!/usr/bin/env bash
# Build one OpenVM guest commitment with checkout-independent compiler paths.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <main|escape> <output-dir>" >&2
    exit 2
fi

case "$1" in
    main)
        manifest="zk/openvm-guest/Cargo.toml"
        config="zk/openvm-guest/openvm.toml"
        ;;
    escape)
        manifest="zk/openvm-escape-guest/Cargo.toml"
        config="zk/openvm-escape-guest/openvm.toml"
        ;;
    *)
        echo "unknown OpenVM guest: $1 (expected main or escape)" >&2
        exit 2
        ;;
esac

output_dir="$2"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
remap_flags="--remap-path-prefix=/__SYBIL_WORKSPACE_ROOT__=/sybil-src --remap-path-prefix=/__SYBIL_CARGO_HOME__=/cargo --remap-path-prefix=/__SYBIL_RUSTUP_HOME__=/rustc"

cd "$REPO_ROOT"
PATH="$REPO_ROOT/scripts:$PATH" \
    SYBIL_WORKSPACE_ROOT="$REPO_ROOT" \
    SYBIL_CARGO_HOME="$cargo_home" \
    SYBIL_RUSTUP_HOME="$rustup_home" \
    RUSTC_WRAPPER="openvm-rustc-wrapper.sh" \
    RUSTFLAGS="$remap_flags${RUSTFLAGS:+ $RUSTFLAGS}" \
    cargo openvm commit \
        --manifest-path "$manifest" \
        --config "$config" \
        --output-dir "$output_dir"
