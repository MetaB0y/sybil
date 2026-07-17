#!/usr/bin/env bash
#
# Regenerate the vendored Sybil OpenAPI Python client.
#
# Mirrors the frontend's `types:generate`: renders the canonical full
# development-superset document with the `sybil-openapi` binary and regenerates
# `arena/sybil_client/_generated` with openapi-python-client.
#
# The hand-written ergonomic layer (`sybil_client/client.py` + `types.py`) is
# NOT touched — only the `_generated/` package. Run it from anywhere:
#
#     just arena-sdk-regen        # or: arena/scripts/regen-sdk.sh
#
# Reproducible: post-generation hooks are disabled (see the --config file), so
# the output is byte-stable across runs regardless of local formatter versions.
#
# Spec source (default: run the deterministic `sybil-openapi` renderer):
#   SYBIL_OPENAPI=path/or/url   Use an existing spec and skip the Rust build+boot
#                               entirely. Mirrors the frontend's
#                               `${NEXT_PUBLIC_API_BASE}/openapi.json` override,
#                               and keeps regen usable when the Rust workspace is
#                               mid-refactor / not compiling.
set -euo pipefail

ARENA_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$ARENA_DIR/.." && pwd)"
OUT_DIR="$ARENA_DIR/sybil_client/_generated"
CONFIG="$ARENA_DIR/scripts/openapi-python-client-config.yml"

# Pin the generator so the vendored tree is reproducible byte-for-byte across
# machines and CI. Bumping this is a deliberate, reviewed change (regenerate and
# commit the diff). Keep in sync with sybil_client/README.md.
GENERATOR_VERSION="0.29.0"

SPEC="$(mktemp -t sybil-openapi.XXXXXX.json)"

cleanup() {
    rm -f "$SPEC"
}
trap cleanup EXIT

if [ -n "${SYBIL_OPENAPI:-}" ]; then
    echo "==> Using spec override: $SYBIL_OPENAPI"
    case "$SYBIL_OPENAPI" in
        http://*|https://*) curl -fsS "$SYBIL_OPENAPI" -o "$SPEC" ;;
        *) cp "$SYBIL_OPENAPI" "$SPEC" ;;
    esac
else
    echo "==> Rendering canonical full OpenAPI document"
    cargo run --quiet \
        -p sybil-api \
        --bin sybil-openapi \
        --manifest-path "$REPO_ROOT/Cargo.toml" >"$SPEC"
fi

if [ ! -s "$SPEC" ]; then
    echo "ERROR: could not obtain a non-empty OpenAPI spec" >&2
    exit 1
fi

echo "==> Regenerating $OUT_DIR"
rm -rf "$OUT_DIR"
uvx "openapi-python-client@${GENERATOR_VERSION}" generate \
    --path "$SPEC" \
    --meta none \
    --output-path "$OUT_DIR" \
    --config "$CONFIG"

echo "==> Done. Vendored client regenerated at sybil_client/_generated"
