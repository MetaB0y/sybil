#!/usr/bin/env bash
# Restore a store backup into a fresh isolated Compose project (SYB-223 item 2).
#
# Usage:
#   scripts/store-restore-drill.sh BACKUP_DIR [--port PORT] [--timeout SECONDS]
#                                                   [--no-build] [--dry-run]
#
# The drill uses docker-compose.yml + docker-compose.itest.yml, a unique project
# and a unique named volume. Cleanup always runs `down -v`; production volumes
# are neither named nor mounted by this script.

set -euo pipefail

BACKUP_DIR=""
PORT="${SYBIL_RESTORE_DRILL_PORT:-3310}"
TIMEOUT=120
BUILD=1
DRY_RUN=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --port) PORT="${2:?missing port}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?missing timeout}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
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
dry-run: create unique Compose project with docker-compose.yml + docker-compose.itest.yml
dry-run: populate only that project's fresh itest-data volume from $BACKUP_DIR/store
dry-run: $BUILD_DESCRIPTION sybil-api with a 24h block interval on 127.0.0.1:$PORT
dry-run: require health and exact manifest matches for latest height/state_root and sampled account
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
if docker compose version >/dev/null 2>&1; then
    COMPOSE_BIN=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_BIN=(docker-compose)
else
    echo "error: neither docker compose nor docker-compose is available" >&2
    exit 2
fi

python3 - "$BACKUP_DIR/manifest.json" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
assert m.get("schema") == "sybil.store-backup.v1"
e = m["expected"]
assert isinstance(e["height"], int) and e["height"] >= 0
assert isinstance(e["state_root"], str) and len(e["state_root"]) == 64
assert isinstance(e["account_id"], int) and e["account_id"] >= 0
assert e["account"]["account_id"] == e["account_id"]
PY
(
    cd "$BACKUP_DIR/store"
    sha256sum -c "$BACKUP_DIR/SHA256SUMS"
) >/dev/null

PROJECT="sybil-restore-drill-$(date +%s)-$$"
export COMPOSE_PROJECT_NAME="$PROJECT"
export SYBIL_ITEST_PORT="$PORT"
export SYBIL_ITEST_BLOCK_INTERVAL_MS=86400000
COMPOSE=("${COMPOSE_BIN[@]}" -p "$PROJECT" -f "$ROOT/docker-compose.yml" -f "$ROOT/docker-compose.itest.yml")
compose() { "${COMPOSE[@]}" "$@"; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/store-restore-drill.XXXXXX")"
cleanup() {
    local status=$?
    trap - EXIT INT TERM
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
trap cleanup EXIT INT TERM

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

python3 - "$BACKUP_DIR/manifest.json" "$WORK/latest.json" \
    "$WORK/state-root.json" "$WORK/account.json" <<'PY'
import json, sys
manifest, latest, state_root, account = [
    json.load(open(path, encoding="utf-8")) for path in sys.argv[1:5]
]
expected = manifest["expected"]
failures = []
if latest.get("height") != expected["height"]:
    failures.append(f"height expected {expected['height']}, got {latest.get('height')}")
if latest.get("state_root") != expected["state_root"]:
    failures.append("latest block state_root mismatch")
if state_root.get("state_root") != expected["state_root"]:
    failures.append("/v1/state-root mismatch")
if account != expected["account"]:
    failures.append(f"account {expected['account_id']} state mismatch")
if failures:
    raise SystemExit("; ".join(failures))
print(f"OK: restored height={expected['height']} state_root={expected['state_root']} account={expected['account_id']}")
PY

echo "RESULT: restored OK — exact manifest state served from an isolated fresh volume"
