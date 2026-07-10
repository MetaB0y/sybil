#!/usr/bin/env bash
# Stabilize rustc's crate-disambiguator metadata for reproducible OpenVM ELFs.
#
# Cargo derives its `-C metadata` value from unit identity, including absolute
# paths for local path dependencies. Path remapping removes those paths from
# the emitted strings, but Cargo's differing metadata still changes symbol
# hashes and code layout. Replace only that salt with a stable package identity;
# keep Cargo's distinct output filenames and every other rustc argument intact.
set -euo pipefail

rustc="$1"
shift

# Cargo must fingerprint the same literal RUSTFLAGS in every checkout. Expand
# the stable sentinels to this machine's actual roots only for the rustc call.
args=("$@")
for i in "${!args[@]}"; do
    case "${args[$i]}" in
        --remap-path-prefix=/__SYBIL_WORKSPACE_ROOT__=/sybil-src)
            args[$i]="--remap-path-prefix=$SYBIL_WORKSPACE_ROOT=/sybil-src"
            ;;
        --remap-path-prefix=/__SYBIL_CARGO_HOME__=/cargo)
            args[$i]="--remap-path-prefix=$SYBIL_CARGO_HOME=/cargo"
            ;;
        --remap-path-prefix=/__SYBIL_RUSTUP_HOME__=/rustc)
            args[$i]="--remap-path-prefix=$SYBIL_RUSTUP_HOME=/rustc"
            ;;
    esac
done

# Registry, toolchain, and build-std crates already have checkout-independent
# identities. Re-salting those can collapse Cargo's intentionally distinct
# build-std/user dependency units (for example two cfg-if units).
case "${CARGO_MANIFEST_DIR:-}" in
    "$SYBIL_WORKSPACE_ROOT"|"$SYBIL_WORKSPACE_ROOT"/*) ;;
    *) exec "$rustc" "${args[@]}" ;;
esac

feature_names=""
while IFS= read -r feature_var; do
    feature_names+="${feature_var#CARGO_FEATURE_},"
done < <(compgen -A variable CARGO_FEATURE_ | LC_ALL=C sort)

identity="${CARGO_PKG_NAME:-unknown}|${CARGO_PKG_VERSION:-0}|${CARGO_CRATE_NAME:-unknown}|${TARGET:-${CARGO_CFG_TARGET_ARCH:-unknown}}|$feature_names"
metadata="sybil$(printf '%s' "$identity" | sha256sum | cut -c1-16)"

for i in "${!args[@]}"; do
    case "${args[$i]}" in
        metadata=*) args[$i]="metadata=$metadata" ;;
        -Cmetadata=*) args[$i]="-Cmetadata=$metadata" ;;
    esac
done

exec "$rustc" "${args[@]}"
