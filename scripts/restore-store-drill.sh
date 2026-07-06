#!/usr/bin/env bash
# Restore-drill for a Sybil store backup (SYB-223).
#
# Proves a backup is RESTORABLE: copies it into a throwaway location, boots a
# fresh sybil-api against it on an ephemeral port (SYBIL_DATA_DIR override),
# waits for health, confirms /v1/blocks/latest and /v1/state-root answer from
# the restored state, then tears everything down. Nothing touches the live
# store or the live port.
#
# What this drill DOES prove:
#   - the backup's redb + qmdb open cleanly under this binary's layout version
#   - recovery passes its invariants (height/fence/root all agree)
#   - the restored state serves a state root and a committed block height
#
# What this drill does NOT prove (see docs/runbooks/store-backup-restore.md):
#   - that the backup is byte-current with production at cutover
#   - historical-serving completeness (pruned history is not reconstructed)
#   - L1 re-indexing / external side-effects; correctness of in-flight orders
#   - anything about a backup taken UNSAFELY (online cp of a live store)
#
# Usage:
#   scripts/restore-store-drill.sh <backup_dir> [--api-binary PATH] [--timeout SECS]
#
#   <backup_dir>   dir containing sybil.redb (+ sybil.qmdb/), e.g. a
#                  scripts/backup-store.sh output directory
#   --api-binary   path to a prebuilt sybil-api (default: cargo build -p sybil-api)
#   --timeout      seconds to wait for health before failing (default: 30)

set -uo pipefail

BACKUP_DIR=""
API_BIN=""
TIMEOUT=30

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --api-binary) API_BIN="${2:?}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?}"; shift 2 ;;
        --*) echo "unknown flag: $1" >&2; usage 2 ;;
        *) if [[ -z "$BACKUP_DIR" ]]; then BACKUP_DIR="$1"; shift
           else echo "unexpected argument: $1" >&2; usage 2; fi ;;
    esac
done

[[ -z "$BACKUP_DIR" ]] && { echo "error: <backup_dir> is required" >&2; usage 2; }
[[ -f "$BACKUP_DIR/sybil.redb" ]] || { echo "error: $BACKUP_DIR/sybil.redb not found" >&2; exit 2; }

for tool in curl python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Locate / build the API binary ───────────────────────────────────────────
if [[ -z "$API_BIN" ]]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "error: cargo not found and --api-binary not given" >&2; exit 2; }
    echo "Building sybil-api (cargo)..."
    cargo build -q --manifest-path "$REPO_ROOT/Cargo.toml" -p sybil-api --bin sybil-api
    API_BIN="$REPO_ROOT/target/debug/sybil-api"
fi
[[ -x "$API_BIN" ]] || { echo "error: sybil-api binary not executable: $API_BIN" >&2; exit 2; }

# ── Throwaway restore location + teardown ───────────────────────────────────
WORK="$(mktemp -d)"
API_PID=""
cleanup() {
    if [[ -n "$API_PID" ]] && kill -0 "$API_PID" 2>/dev/null; then
        kill "$API_PID" 2>/dev/null || true
        wait "$API_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

RESTORE_DIR="$WORK/data"
mkdir -p "$RESTORE_DIR"
echo "Restoring backup into throwaway dir: $RESTORE_DIR"
cp -a "$BACKUP_DIR/." "$RESTORE_DIR/"
# Drop any copied manifest so only store files remain.
rm -f "$RESTORE_DIR/BACKUP_MANIFEST.txt"

# ── Ephemeral free port ─────────────────────────────────────────────────────
PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
BASE="http://127.0.0.1:$PORT"
echo "Starting sybil-api on $BASE against restored store..."

SYBIL_DATA_DIR="$RESTORE_DIR" SYBIL_PORT="$PORT" SYBIL_DEV_MODE=false \
    "$API_BIN" >"$WORK/api.log" 2>&1 &
API_PID=$!

# ── Wait for health ─────────────────────────────────────────────────────────
deadline=$(( $(date +%s) + TIMEOUT ))
healthy=0
while [[ "$(date +%s)" -lt "$deadline" ]]; do
    if ! kill -0 "$API_PID" 2>/dev/null; then
        echo "error: sybil-api exited during startup. Last log lines:" >&2
        tail -30 "$WORK/api.log" >&2
        exit 5
    fi
    if curl -sS -m 5 -o /dev/null -w '%{http_code}' "$BASE/v1/health" 2>/dev/null | grep -q '^2'; then
        healthy=1
        break
    fi
    sleep 1
done

if [[ "$healthy" -ne 1 ]]; then
    echo "FAIL: sybil-api did not become healthy within ${TIMEOUT}s. Last log lines:" >&2
    tail -30 "$WORK/api.log" >&2
    exit 5
fi
echo "[PASS] /v1/health OK"

# ── Confirm restored state serves ───────────────────────────────────────────
FAILN=0

BLK="$(curl -sS -m 5 "$BASE/v1/blocks/latest" 2>/dev/null)"
HEIGHT="$(echo "$BLK" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("height",""))' 2>/dev/null)"
if [[ -n "$HEIGHT" ]]; then
    echo "[PASS] /v1/blocks/latest restored at height=$HEIGHT"
else
    echo "[FAIL] /v1/blocks/latest did not return a height: $BLK"; FAILN=$((FAILN+1))
fi

SR="$(curl -sS -m 5 "$BASE/v1/state-root" 2>/dev/null)"
ROOT="$(echo "$SR" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("state_root",""))' 2>/dev/null)"
if [[ -n "$ROOT" ]]; then
    echo "[PASS] /v1/state-root restored: ${ROOT:0:16}..."
else
    echo "[FAIL] /v1/state-root did not return a root: $SR"; FAILN=$((FAILN+1))
fi

echo "Tearing down drill instance..."
if [[ "$FAILN" -gt 0 ]]; then
    echo "RESULT: FAIL"
    exit 1
fi
echo "RESULT: OK — backup is restorable"
exit 0
