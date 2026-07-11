#!/usr/bin/env bash
# Crash-consistent hot backup of the running Sybil store (SYB-223 item 2).
#
# Usage:
#   scripts/store-backup.sh --target prod [--dest DIR] [--account-id ID]
#   scripts/store-backup.sh --target itest --project PROJECT [--dest DIR]
#   scripts/store-backup.sh --target custom --container NAME --data-dir DIR
#                           [--dest DIR] [--account-id ID]
#   scripts/store-backup.sh ... --dry-run
#
# The source container is frozen with `docker pause` while the complete data
# directory is copied. The service is always unpaused by the EXIT trap. A
# throwaway inspector is then booted from a second copy to record the exact
# restored height, state root, and one complete account response in manifest.json.

set -euo pipefail

TARGET=""
PROJECT=""
CONTAINER=""
DATA_DIR=""
DEST=""
ACCOUNT_ID=""
TIMEOUT=90
DRY_RUN=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --target) TARGET="${2:?missing target}"; shift 2 ;;
        --project) PROJECT="${2:?missing project}"; shift 2 ;;
        --container) CONTAINER="${2:?missing container}"; shift 2 ;;
        --data-dir) DATA_DIR="${2:?missing data directory}"; shift 2 ;;
        --dest) DEST="${2:?missing destination}"; shift 2 ;;
        --account-id) ACCOUNT_ID="${2:?missing account id}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?missing timeout}"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        *) echo "unknown argument: $1" >&2; usage 2 ;;
    esac
done

[[ "$TARGET" == "prod" || "$TARGET" == "itest" || "$TARGET" == "custom" ]] \
    || { echo "error: --target must be prod, itest, or custom" >&2; exit 2; }
[[ -z "$ACCOUNT_ID" || "$ACCOUNT_ID" =~ ^[0-9]+$ ]] \
    || { echo "error: --account-id must be an unsigned integer" >&2; exit 2; }
[[ "$TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { echo "error: --timeout must be positive" >&2; exit 2; }

case "$TARGET" in
    prod)
        PROJECT="${PROJECT:-sybil}"
        DATA_DIR="${DATA_DIR:-/data}"
        DEST="${DEST:-/opt/sybil/backups}"
        ;;
    itest)
        [[ -n "$PROJECT" || -n "$CONTAINER" ]] \
            || { echo "error: itest target requires --project or --container" >&2; exit 2; }
        DATA_DIR="${DATA_DIR:-/itest-data}"
        DEST="${DEST:-./store-backups}"
        ;;
    custom)
        [[ -n "$CONTAINER" ]] || { echo "error: custom target requires --container" >&2; exit 2; }
        [[ -n "$DATA_DIR" ]] || { echo "error: custom target requires --data-dir" >&2; exit 2; }
        DEST="${DEST:-./store-backups}"
        ;;
esac

if [[ "$DRY_RUN" -eq 1 ]]; then
    cat <<EOF
dry-run: resolve running sybil-api container ${CONTAINER:-from compose project '$PROJECT'}
dry-run: verify $DATA_DIR/sybil.redb and $DATA_DIR/sybil.qmdb exist in the source
dry-run: docker pause <source>; docker cp <source>:$DATA_DIR/. <timestamped-backup>/store/; docker unpause <source>
dry-run: hash every copied file and boot the source image against a throwaway second copy
dry-run: record exact restored height/state_root and account ${ACCOUNT_ID:-auto (leaderboard, then account 0)} in <timestamped-backup>/manifest.json
dry-run: destination root $DEST; the production sybil-data volume is never mounted or modified
EOF
    exit 0
fi

for tool in docker python3 sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done

if [[ -z "$CONTAINER" ]]; then
    mapfile -t CANDIDATES < <(docker ps -q \
        --filter "label=com.docker.compose.project=$PROJECT" \
        --filter 'label=com.docker.compose.service=sybil-api')
    [[ "${#CANDIDATES[@]}" -eq 1 ]] || {
        echo "error: expected one running sybil-api container in project '$PROJECT', found ${#CANDIDATES[@]}" >&2
        exit 2
    }
    CONTAINER="${CANDIDATES[0]}"
fi

RUNNING="$(docker inspect --format '{{.State.Running}}' "$CONTAINER")"
PAUSED_STATE="$(docker inspect --format '{{.State.Paused}}' "$CONTAINER")"
[[ "$RUNNING" == "true" && "$PAUSED_STATE" == "false" ]] \
    || { echo "error: source container must be running and unpaused" >&2; exit 2; }
docker exec "$CONTAINER" test -f "$DATA_DIR/sybil.redb" \
    || { echo "error: $DATA_DIR/sybil.redb not found in $CONTAINER" >&2; exit 2; }
docker exec "$CONTAINER" test -d "$DATA_DIR/sybil.qmdb" \
    || { echo "error: $DATA_DIR/sybil.qmdb not found in $CONTAINER" >&2; exit 2; }

SOURCE_IMAGE="$(docker inspect --format '{{.Config.Image}}' "$CONTAINER")"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$DEST/sybil-store-$STAMP-$$"
INSPECT_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/store-backup-inspect.XXXXXX")"
INSPECT_CONTAINER="store-backup-inspect-$STAMP-$$"
SOURCE_PAUSED=0
COMPLETE=0

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    docker rm -f "$INSPECT_CONTAINER" >/dev/null 2>&1 || true
    rm -rf "$INSPECT_ROOT"
    if [[ "$SOURCE_PAUSED" -eq 1 ]]; then
        if ! docker unpause "$CONTAINER" >/dev/null; then
            echo "CRITICAL: failed to unpause source container $CONTAINER" >&2
            status=1
        fi
    fi
    if [[ "$COMPLETE" -ne 1 ]]; then
        rm -rf "$OUT"
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

mkdir -p "$OUT/store"
echo "Freezing $CONTAINER for a crash-consistent copy..."
docker pause "$CONTAINER" >/dev/null
SOURCE_PAUSED=1
docker cp "$CONTAINER:$DATA_DIR/." "$OUT/store/"
docker unpause "$CONTAINER" >/dev/null
SOURCE_PAUSED=0
echo "Source resumed; validating the copied store in isolation..."

[[ -s "$OUT/store/sybil.redb" && -d "$OUT/store/sybil.qmdb" ]] \
    || { echo "error: copied backup is missing sybil.redb or sybil.qmdb" >&2; exit 3; }
(
    cd "$OUT/store"
    find . -type f -print0 | sort -z | xargs -0 sha256sum
) > "$OUT/SHA256SUMS"

mkdir -p "$INSPECT_ROOT/data"
cp -a "$OUT/store/." "$INSPECT_ROOT/data/"
docker run -d --name "$INSPECT_CONTAINER" --network none \
    -v "$INSPECT_ROOT/data:/data" \
    -e SYBIL_DEPLOYMENT_PROFILE=local \
    -e SYBIL_DEV_MODE=true \
    -e SYBIL_DATA_DIR=/data \
    -e SYBIL_PORT=3000 \
    -e SYBIL_BLOCK_INTERVAL_MS=86400000 \
    -e SYBIL_ARENA_DB_PATH= \
    -e SYBIL_EVENT_SNAPSHOT_DIR= \
    -e SYBIL_MARKET_REF_DATA_PATH= \
    --entrypoint sybil-api "$SOURCE_IMAGE" >/dev/null

READY=0
for _ in $(seq 1 "$TIMEOUT"); do
    if docker exec "$INSPECT_CONTAINER" curl -fsS --max-time 3 \
        http://127.0.0.1:3000/v1/health >/dev/null 2>&1; then
        READY=1
        break
    fi
    [[ "$(docker inspect --format '{{.State.Running}}' "$INSPECT_CONTAINER" 2>/dev/null || true)" == "true" ]] || break
    sleep 1
done
if [[ "$READY" -ne 1 ]]; then
    echo "error: copied store did not boot in the inspector container" >&2
    docker logs --tail 40 "$INSPECT_CONTAINER" >&2 || true
    exit 4
fi

docker exec "$INSPECT_CONTAINER" curl -fsS http://127.0.0.1:3000/v1/blocks/latest \
    > "$INSPECT_ROOT/latest.json"
docker exec "$INSPECT_CONTAINER" curl -fsS http://127.0.0.1:3000/v1/state-root \
    > "$INSPECT_ROOT/state-root.json"

if [[ -z "$ACCOUNT_ID" ]]; then
    docker exec "$INSPECT_CONTAINER" curl -fsS \
        'http://127.0.0.1:3000/v1/leaderboard?window=all&limit=1' \
        > "$INSPECT_ROOT/leaderboard.json"
    ACCOUNT_ID="$(python3 - "$INSPECT_ROOT/leaderboard.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
entries = value.get("entries", []) if isinstance(value, dict) else []
print(entries[0]["account_id"] if entries else "")
PY
)"
    [[ -n "$ACCOUNT_ID" ]] || ACCOUNT_ID=0
fi
if ! docker exec "$INSPECT_CONTAINER" curl -fsS \
    "http://127.0.0.1:3000/v1/accounts/$ACCOUNT_ID" > "$INSPECT_ROOT/account.json"; then
    echo "error: no sample account found; pass --account-id with an existing account" >&2
    exit 4
fi

python3 - "$INSPECT_ROOT/latest.json" "$INSPECT_ROOT/state-root.json" \
    "$INSPECT_ROOT/account.json" "$OUT/manifest.json" "$STAMP" "$TARGET" \
    "$PROJECT" "$CONTAINER" "$SOURCE_IMAGE" "$DATA_DIR" <<'PY'
import json, socket, sys
latest_path, root_path, account_path, output = sys.argv[1:5]
stamp, target, project, container, image, data_dir = sys.argv[5:11]
latest = json.load(open(latest_path, encoding="utf-8"))
root = json.load(open(root_path, encoding="utf-8"))
account = json.load(open(account_path, encoding="utf-8"))
height = latest.get("height")
block_root = latest.get("state_root")
served_root = root.get("state_root")
if not isinstance(height, int) or not block_root or block_root != served_root:
    raise SystemExit("inspector returned inconsistent latest block/state root")
if not isinstance(account, dict) or not isinstance(account.get("account_id"), int):
    raise SystemExit("inspector returned an invalid account sample")
manifest = {
    "schema": "sybil.store-backup.v1",
    "created_utc": stamp,
    "host": socket.gethostname(),
    "source": {
        "target": target,
        "compose_project": project or None,
        "container": container,
        "image": image,
        "data_dir": data_dir,
    },
    "consistency": {
        "mechanism": "docker-pause-whole-container",
        "scope": "complete-sybil-data-dir",
    },
    "expected": {
        "height": height,
        "state_root": block_root,
        "account_id": account["account_id"],
        "account": account,
    },
}
with open(output, "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")
PY

docker rm -f "$INSPECT_CONTAINER" >/dev/null
rm -rf "$INSPECT_ROOT"
COMPLETE=1
echo "OK: backup restored in isolation at height=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["expected"]["height"])' "$OUT/manifest.json")"
echo "$OUT"
