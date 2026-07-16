#!/usr/bin/env bash
# Restore a store backup into a fresh isolated Compose project (SYB-223 item 2).
#
# Usage:
#   scripts/store-restore-drill.sh BACKUP_DIR [--port PORT] [--timeout SECONDS]
#                               [--no-build] [--allow-live-host] [--dry-run]
#
# The drill uses standalone docker-compose.itest.yml, a unique project, and a
# unique named volume. Cleanup always runs `down -v`; production volumes are
# neither named nor mounted by this script.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BACKUP_DIR=""
PORT="${SYBIL_RESTORE_DRILL_PORT:-3310}"
TIMEOUT=120
BUILD=1
DRY_RUN=0
ALLOW_LIVE_HOST=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --port) PORT="${2:?missing port}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?missing timeout}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        --allow-live-host) ALLOW_LIVE_HOST=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --*) echo "unknown argument: $1" >&2; usage 2 ;;
        *)
            [[ -z "$BACKUP_DIR" ]] || { echo "unexpected argument: $1" >&2; usage 2; }
            BACKUP_DIR=$1
            shift
            ;;
    esac
done

[[ -n "$BACKUP_DIR" ]] || { echo "error: BACKUP_DIR is required" >&2; usage 2; }
[[ "$PORT" =~ ^[1-9][0-9]{0,4}$ && "$PORT" -le 65535 ]] \
    || { echo "error: invalid port '$PORT'" >&2; exit 2; }
[[ "$TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { echo "error: timeout must be positive" >&2; exit 2; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$DRY_RUN" -eq 1 ]]; then
    if [[ "$BUILD" -eq 1 ]]; then BUILD_DESCRIPTION="build and start"; else BUILD_DESCRIPTION="start existing image for"; fi
    cat <<EOF
dry-run: validate $BACKUP_DIR/manifest.json and SHA256SUMS
dry-run: refuse a Docker daemon serving sybil-data unless --allow-live-host was explicit
dry-run: create a unique project from standalone docker-compose.itest.yml only
dry-run: populate only that project's fresh itest-data volume from $BACKUP_DIR/store
dry-run: $BUILD_DESCRIPTION sybil-api with a 24h block interval on 127.0.0.1:$PORT
dry-run: require health and exact manifest matches for latest height, committed/replayed state roots, and sampled account
dry-run: docker compose down -v --remove-orphans (production sybil-data is never referenced)
EOF
    exit 0
fi

BACKUP_DIR="$(cd "$BACKUP_DIR" 2>/dev/null && pwd)" \
    || { echo "error: backup directory not found" >&2; exit 2; }

[[ -f "$BACKUP_DIR/manifest.json" && -f "$BACKUP_DIR/SHA256SUMS" \
    && -s "$BACKUP_DIR/store/sybil.redb" && -d "$BACKUP_DIR/store/sybil.qmdb" ]] \
    || { echo "error: backup is missing manifest, checksums, redb, or qMDB" >&2; exit 2; }

for tool in docker curl python3 sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done
[[ -f "$SCRIPT_DIR/store-manifest.py" ]] \
    || { echo "error: $SCRIPT_DIR/store-manifest.py is required" >&2; exit 2; }
if docker compose version >/dev/null 2>&1; then
    COMPOSE_BIN=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_BIN=(docker-compose)
else
    echo "error: neither docker compose nor docker-compose is available" >&2
    exit 2
fi

# A second full-state recovery can exhaust the small production host even
# though its volume is logically isolated. More importantly, restore drills
# should never share a Docker resource boundary with a live authoritative API
# accidentally. Operators with measured headroom must opt in explicitly.
if [[ "$ALLOW_LIVE_HOST" -ne 1 ]]; then
    mapfile -t LIVE_APIS < <(docker ps -q \
        --filter 'label=com.docker.compose.service=sybil-api' \
        --filter 'volume=sybil-data')
    if [[ "${#LIVE_APIS[@]}" -ne 0 ]]; then
        echo "error: refusing restore drill on a Docker daemon with a live sybil-data API; use a separate host or pass --allow-live-host deliberately" >&2
        exit 2
    fi
fi

python3 "$SCRIPT_DIR/store-manifest.py" validate "$BACKUP_DIR/manifest.json"
(
    cd "$BACKUP_DIR/store"
    sha256sum -c "$BACKUP_DIR/SHA256SUMS"
) >/dev/null

PROJECT="sybil-restore-drill-$(date +%s)-$$"
export COMPOSE_PROJECT_NAME="$PROJECT"
export SYBIL_ITEST_PORT="$PORT"
export SYBIL_ITEST_BLOCK_INTERVAL_MS=86400000
# docker-compose.itest.yml is intentionally complete enough for this one
# service. Merging the base file would append sybil-data:/data and expose the
# globally named production volume to `down -v` cleanup.
COMPOSE=("${COMPOSE_BIN[@]}" -p "$PROJECT" -f "$ROOT/docker-compose.itest.yml")
compose() { "${COMPOSE[@]}" "$@"; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/store-restore-drill.XXXXXX")"
cleanup() {
    local status=${1:-$?}
    trap - EXIT HUP INT TERM
    if [[ "$status" -ne 0 ]]; then
        compose logs --no-color sybil-api > "$WORK/sybil-api.log" 2>&1 || true
        if [[ -s "$WORK/sybil-api.log" ]]; then
            echo "last restored-node log lines:" >&2
            tail -40 "$WORK/sybil-api.log" >&2
        fi
    fi
    compose down -v --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$WORK"
    exit "$status"
}
trap cleanup EXIT
trap 'cleanup 129' HUP
trap 'cleanup 130' INT
trap 'cleanup 143' TERM

if [[ "$BUILD" -eq 1 ]]; then
    compose build sybil-api
fi

# Compose run receives only the unique itest-data volume declared by the
# overlay. The bind is read-only; cp writes into the throwaway named volume.
compose run --rm --no-deps --entrypoint sh \
    -v "$BACKUP_DIR/store:/backup:ro" sybil-api \
    -c 'cp -a /backup/. /itest-data/'
compose up -d --no-build sybil-api

BASE="http://127.0.0.1:$PORT"
READY=0
for _ in $(seq 1 "$TIMEOUT"); do
    if curl -fsS --max-time 3 "$BASE/v1/health" > "$WORK/health.json" 2>/dev/null; then
        READY=1
        break
    fi
    sleep 1
done
[[ "$READY" -eq 1 ]] || { echo "FAIL: restored node did not become healthy" >&2; exit 4; }

curl -fsS --max-time 10 "$BASE/v1/blocks/latest" > "$WORK/latest.json"
curl -fsS --max-time 10 "$BASE/v1/state-root" > "$WORK/state-root.json"
ACCOUNT_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["expected"]["account_id"])' "$BACKUP_DIR/manifest.json")"
curl -fsS --max-time 10 "$BASE/v1/accounts/$ACCOUNT_ID" > "$WORK/account.json"

python3 "$SCRIPT_DIR/store-manifest.py" compare \
    --manifest "$BACKUP_DIR/manifest.json" \
    --latest "$WORK/latest.json" \
    --state-root "$WORK/state-root.json" \
    --account "$WORK/account.json"

echo "RESULT: restored OK — exact manifest state served from an isolated fresh volume"
