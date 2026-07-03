#!/usr/bin/env bash
#
# Regenerate the vendored Sybil OpenAPI Python client.
#
# Mirrors the frontend's `types:generate` (openapi-typescript against the live
# spec): builds and boots `sybil-api` on a free port, fetches /openapi.json, and
# regenerates `arena/sybil_client/_generated` from it with openapi-python-client.
#
# The hand-written ergonomic layer (`sybil_client/client.py` + `types.py`) is
# NOT touched — only the `_generated/` package. Run it from anywhere:
#
#     just arena-sdk-regen        # or: arena/scripts/regen-sdk.sh
#
# Reproducible: post-generation hooks are disabled (see the --config file), so
# the output is byte-stable across runs regardless of local formatter versions.
#
# Spec source (default: build + boot sybil-api locally and scrape /openapi.json):
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

SPEC="$(mktemp -t sybil-openapi.XXXXXX.json)"
SERVER_PID=""

cleanup() {
    [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
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
    # Pick a free TCP port so we never collide with a running dev server.
    PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"

    echo "==> Building sybil-api"
    cargo build -p sybil-api --manifest-path "$REPO_ROOT/Cargo.toml"

    echo "==> Booting sybil-api on port $PORT (in-memory defaults)"
    SYBIL_PORT="$PORT" "$REPO_ROOT/target/debug/sybil-api" >/dev/null 2>&1 &
    SERVER_PID=$!

    echo "==> Waiting for /openapi.json"
    for _ in $(seq 1 60); do
        if curl -fsS "http://127.0.0.1:$PORT/openapi.json" -o "$SPEC" 2>/dev/null; then
            break
        fi
        sleep 0.5
    done
fi

if [ ! -s "$SPEC" ]; then
    echo "ERROR: could not obtain a non-empty OpenAPI spec" >&2
    exit 1
fi

echo "==> Regenerating $OUT_DIR"
rm -rf "$OUT_DIR"
uvx openapi-python-client generate \
    --path "$SPEC" \
    --meta none \
    --output-path "$OUT_DIR" \
    --config "$CONFIG"

echo "==> Done. Vendored client regenerated at sybil_client/_generated"
