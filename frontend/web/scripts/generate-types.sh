#!/usr/bin/env bash
set -euo pipefail

WEB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$WEB_DIR/../.." && pwd)"
SCHEMA="$WEB_DIR/src/lib/api/schema.d.ts"
SPEC="$(mktemp -t sybil-openapi.XXXXXX.json)"
GENERATED=""
GENERATED_DIR=""

cleanup() {
    rm -f "$SPEC"
    if [ -n "$GENERATED" ]; then
        rm -f "$GENERATED"
    fi
    if [ -n "$GENERATED_DIR" ]; then
        rmdir "$GENERATED_DIR"
    fi
}
trap cleanup EXIT

cargo run --quiet --manifest-path "$REPO_ROOT/Cargo.toml" \
    -p sybil-api --bin sybil-openapi >"$SPEC"

if [ "${1:-}" = "--check" ]; then
    # macOS mktemp appends its random suffix after the full template, which
    # turns `*.d.ts` into `*.d.ts.<random>` and makes Prettier lose the parser.
    GENERATED_DIR="$(mktemp -d -t sybil-schema.XXXXXX)"
    GENERATED="$GENERATED_DIR/schema.d.ts"
    OUTPUT="$GENERATED"
else
    OUTPUT="$SCHEMA"
fi

openapi-typescript "$SPEC" -o "$OUTPUT"
node "$WEB_DIR/scripts/patch-bigints.mjs" "$OUTPUT"
prettier --write "$OUTPUT"

if [ "${1:-}" = "--check" ] && ! cmp -s "$SCHEMA" "$OUTPUT"; then
    echo "ERROR: src/lib/api/schema.d.ts is stale; run pnpm types:generate" >&2
    diff -u "$SCHEMA" "$OUTPUT" || true
    exit 1
fi
