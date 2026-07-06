#!/usr/bin/env bash
# Back up the Sybil sequencer store (SYB-223).
#
# ── Copy-safety (grounded in matching-sequencer/src/store.rs::Store::open) ───
# The store is TWO on-disk objects, both living under SYBIL_DATA_DIR:
#   - sybil.redb    a single redb v4 file. redb is a copy-on-write B-tree with
#                   MVCC and NO separate write-ahead log; the commit is the only
#                   durability point (store.rs module docs).
#   - sybil.qmdb/   the qmdb account-state directory. store.rs derives it as
#                   `path.with_extension("qmdb")`, i.e. a sibling of sybil.redb.
#
# There is intentionally NO cross-db transaction between redb and qmdb: the redb
# commit fence is authoritative and recovery REQUIRES the fenced qmdb slot to
# match the committed height/root (see the "Recovery invariants" in store.rs).
# redb v4 also exposes no online/hot-backup API. Consequences:
#
#   * STOPPED-COPY is ALWAYS safe — stop sybil-api, then copy the whole data dir.
#     A stopped store has a settled redb fence and a matching qmdb slot.
#   * An ATOMIC FILESYSTEM SNAPSHOT (LVM / ZFS / btrfs) of the data dir is also
#     safe while running: it captures redb + qmdb at one instant, preserving the
#     fence↔slot agreement. Snapshot the dir, then run this against the snapshot.
#   * A plain online `cp` of a LIVE store is NOT safe: it can capture a torn redb
#     page, or a redb fence that points past what the qmdb copy captured — a
#     backup that fails the recovery invariants.
#
# This script therefore refuses to copy a store that still has an open writer
# unless you explicitly assert safety with --assume-stopped or --allow-online.
#
# Usage:
#   scripts/backup-store.sh [--data-dir DIR] [--dest DIR]
#                           [--assume-stopped] [--allow-online]
#
#   --data-dir       store data dir (default: $SYBIL_DATA_DIR)
#   --dest           backup destination root (default: ./store-backups)
#   --assume-stopped you have stopped sybil-api; proceed with a plain copy
#   --allow-online   the source is an atomic FS snapshot (or you accept the risk)

set -euo pipefail

DATA_DIR="${SYBIL_DATA_DIR:-}"
DEST="./store-backups"
ASSUME_STOPPED=0
ALLOW_ONLINE=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --data-dir) DATA_DIR="${2:?}"; shift 2 ;;
        --dest) DEST="${2:?}"; shift 2 ;;
        --assume-stopped) ASSUME_STOPPED=1; shift ;;
        --allow-online) ALLOW_ONLINE=1; shift ;;
        *) echo "unknown argument: $1" >&2; usage 2 ;;
    esac
done

[[ -z "$DATA_DIR" ]] && { echo "error: --data-dir or SYBIL_DATA_DIR is required" >&2; exit 2; }
[[ -d "$DATA_DIR" ]] || { echo "error: data dir not found: $DATA_DIR" >&2; exit 2; }

REDB="$DATA_DIR/sybil.redb"
QMDB="$DATA_DIR/sybil.qmdb"
[[ -f "$REDB" ]] || { echo "error: $REDB not found — is this a Sybil data dir?" >&2; exit 2; }

# ── Writer detection ────────────────────────────────────────────────────────
writer_open() {
    # Returns 0 if some process currently has the redb file open.
    if command -v fuser >/dev/null 2>&1; then
        fuser "$REDB" >/dev/null 2>&1 && return 0
        return 1
    fi
    if command -v lsof >/dev/null 2>&1; then
        lsof -- "$REDB" >/dev/null 2>&1 && return 0
        return 1
    fi
    return 2 # cannot determine
}

if [[ "$ALLOW_ONLINE" -eq 0 && "$ASSUME_STOPPED" -eq 0 ]]; then
    if writer_open; then
        rc=0
    else
        rc=$?
    fi
    case "$rc" in
        0)
            echo "error: $REDB has an open writer — sybil-api still appears to be running." >&2
            echo "       Stop it and re-run, or pass --assume-stopped, or snapshot the" >&2
            echo "       filesystem and pass --allow-online. Plain online copies are unsafe." >&2
            exit 3
            ;;
        2)
            echo "error: cannot verify the store is quiescent (no fuser/lsof installed)." >&2
            echo "       Re-run with --assume-stopped once sybil-api is stopped." >&2
            exit 3
            ;;
    esac
    echo "No open writer detected on $REDB — proceeding with stopped-copy."
fi

# ── Copy ────────────────────────────────────────────────────────────────────
STAMP="$(date -u +%Y%m%d-%H%M%SZ)"
OUT="$DEST/sybil-store-$STAMP"
mkdir -p "$OUT"

echo "Backing up store:"
echo "  from: $DATA_DIR"
echo "  to:   $OUT"

# cp -a preserves timestamps/permissions and copies the redb file plus the
# whole qmdb directory tree. Copy contents (trailing /.) so OUT mirrors the
# data dir layout that Store::open expects.
cp -a "$DATA_DIR/." "$OUT/"

[[ -f "$OUT/sybil.redb" ]] || { echo "error: backup missing sybil.redb" >&2; exit 4; }
if [[ -d "$QMDB" && ! -d "$OUT/sybil.qmdb" ]]; then
    echo "error: backup missing sybil.qmdb (qmdb account state)" >&2; exit 4
fi

# Manifest for provenance / restore drills.
{
    echo "source_data_dir=$DATA_DIR"
    echo "created_utc=$STAMP"
    echo "host=$(hostname)"
    echo "redb_bytes=$(wc -c < "$OUT/sybil.redb")"
    echo "strategy=$([[ "$ALLOW_ONLINE" -eq 1 ]] && echo online-snapshot || echo stopped-copy)"
} > "$OUT/BACKUP_MANIFEST.txt"

REDB_SIZE="$(du -h "$OUT/sybil.redb" | cut -f1)"
echo "OK: backup complete (sybil.redb ${REDB_SIZE})"
echo "$OUT"
