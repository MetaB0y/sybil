#!/usr/bin/env bash
# Restore a store backup into a fresh isolated Compose project (SYB-223 item 2).
#
# Usage:
#   scripts/store-restore-drill.sh BACKUP_DIR [--port PORT] [--timeout SECONDS]
#                               [--retain-validity-artifacts true|false]
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
RETAIN_VALIDITY_ARTIFACTS=""

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --port) PORT="${2:?missing port}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?missing timeout}"; shift 2 ;;
        --retain-validity-artifacts)
            RETAIN_VALIDITY_ARTIFACTS="${2:?missing validity-artifact retention mode}"
            shift 2
            ;;
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
case "$RETAIN_VALIDITY_ARTIFACTS" in
    ""|true|false) ;;
    *) echo "error: --retain-validity-artifacts must be true or false" >&2; exit 2 ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$DRY_RUN" -eq 1 ]]; then
    if [[ "$BUILD" -eq 1 ]]; then BUILD_DESCRIPTION="build and start"; else BUILD_DESCRIPTION="start existing image for"; fi
    cat <<EOF
dry-run: validate $BACKUP_DIR/manifest.json and SHA256SUMS
dry-run: refuse a Docker daemon serving sybil-data unless --allow-live-host was explicit
dry-run: create a unique project from standalone docker-compose.itest.yml only
dry-run: populate only that project's fresh itest-data volume from $BACKUP_DIR/store
dry-run: read the chain's validity-artifact retention mode from the manifest (legacy manifests require an explicit override)
dry-run: $BUILD_DESCRIPTION sybil-api with that exact chain mode and a 24h block interval on 127.0.0.1:$PORT
dry-run: require health and exact manifest matches for latest height, committed/replayed state roots, and sampled account
dry-run: replace only that isolated API against the same throwaway volume with a 1s block interval and require the head to advance while preserving the recorded base block
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
if MANIFEST_RETENTION_MODE="$(
    python3 "$SCRIPT_DIR/store-manifest.py" retention-mode \
        "$BACKUP_DIR/manifest.json" 2>&1
)"; then
    if [[ -n "$RETAIN_VALIDITY_ARTIFACTS" \
        && "$RETAIN_VALIDITY_ARTIFACTS" != "$MANIFEST_RETENTION_MODE" ]]; then
        echo "error: explicit validity-artifact retention mode conflicts with the manifest" >&2
        exit 2
    fi
    RETAIN_VALIDITY_ARTIFACTS="$MANIFEST_RETENTION_MODE"
elif [[ -z "$RETAIN_VALIDITY_ARTIFACTS" ]]; then
    echo "error: $MANIFEST_RETENTION_MODE" >&2
    exit 2
fi
(
    cd "$BACKUP_DIR/store"
    sha256sum -c "$BACKUP_DIR/SHA256SUMS"
) >/dev/null

PROJECT="sybil-restore-drill-$(date +%s)-$$"
export COMPOSE_PROJECT_NAME="$PROJECT"
export SYBIL_ITEST_PORT="$PORT"
export SYBIL_ITEST_BLOCK_INTERVAL_MS=86400000
export SYBIL_ITEST_RETAIN_VALIDITY_ARTIFACTS="$RETAIN_VALIDITY_ARTIFACTS"
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

EXPECTED_HEIGHT="$(
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["expected"]["height"])' \
        "$BACKUP_DIR/manifest.json"
)"

# Exact comparisons must happen before block production resumes. Then replace
# only this unique project's API against the same throwaway volume and require
# a later committed head. Removing the disposable service container before
# starting it again also avoids the legacy Compose v1 ContainerConfig recreate
# failure while retaining the project's named data volume. This proves that
# recovery did not merely open and serve a frozen snapshot: the restored
# sequencer can continue from it.
export SYBIL_ITEST_BLOCK_INTERVAL_MS=1000
compose rm -s -f sybil-api
compose up -d --no-deps --no-build sybil-api
CONTINUED_HEIGHT=""
for _ in $(seq 1 "$TIMEOUT"); do
    if curl -fsS --max-time 3 "$BASE/v1/health" > "$WORK/continued-health.json" 2>/dev/null \
        && curl -fsS --max-time 3 "$BASE/v1/blocks/latest" > "$WORK/continued-latest.json" 2>/dev/null; then
        CONTINUED_HEIGHT="$(
            python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("height", ""))' \
                "$WORK/continued-latest.json" 2>/dev/null || true
        )"
        if [[ "$CONTINUED_HEIGHT" =~ ^[0-9]+$ && "$CONTINUED_HEIGHT" -gt "$EXPECTED_HEIGHT" ]]; then
            break
        fi
    fi
    sleep 1
done
[[ "$CONTINUED_HEIGHT" =~ ^[0-9]+$ && "$CONTINUED_HEIGHT" -gt "$EXPECTED_HEIGHT" ]] \
    || { echo "FAIL: restored node did not advance beyond height $EXPECTED_HEIGHT" >&2; exit 4; }

curl -fsS --max-time 10 "$BASE/v1/blocks/$EXPECTED_HEIGHT" > "$WORK/continued-base.json"
python3 - "$BACKUP_DIR/manifest.json" "$WORK/continued-base.json" <<'PY'
import json
import sys

manifest_path, block_path = sys.argv[1:]
with open(manifest_path, encoding="utf-8") as handle:
    expected = json.load(handle)["expected"]
with open(block_path, encoding="utf-8") as handle:
    block = json.load(handle)
if block.get("height") != expected["height"]:
    raise SystemExit("continued chain did not retain the recorded base height")
committed_state_root = expected.get("committed_state_root", expected.get("state_root"))
if block.get("state_root") != committed_state_root:
    raise SystemExit("continued chain changed the recorded base state root")
PY

echo "RESULT: restored OK — exact manifest state served and continued from height $EXPECTED_HEIGHT to $CONTINUED_HEIGHT in an isolated fresh volume"
