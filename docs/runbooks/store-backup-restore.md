# Runbook: Sequencer store backup & restore

**Owning ticket:** SYB-223 (item 2) ·
**Components:** `matching-sequencer` (store), `sybil-api`, ops ·
**Scripts:** `scripts/backup-store.sh`, `scripts/restore-store-drill.sh`

The Sybil sequencer persists all durable state under `SYBIL_DATA_DIR`. This
runbook covers backing that state up safely and drilling that a backup is
actually restorable.

---

## What is on disk

`matching-sequencer/src/store.rs` (`Store::open`) opens **two** objects, both
inside `SYBIL_DATA_DIR`:

| Path | What | Notes |
|------|------|-------|
| `sybil.redb` | single redb v4 file | copy-on-write B-tree, MVCC, **no** separate WAL; the redb commit is the only durability point |
| `sybil.qmdb/` | qmdb account-state directory | derived as `path.with_extension("qmdb")`, i.e. a sibling of `sybil.redb` |

The redb commit fence is authoritative. There is **no cross-db transaction**
between redb and qmdb: recovery trusts the redb fence and *requires* the fenced
qmdb slot to match the committed height and state root (see the "Recovery
invariants" block in `store.rs`). A backup must therefore capture **both**
objects at a **single consistent instant**.

---

## Copy-safety

redb v4 exposes no online/hot-backup API, and because redb + qmdb are two stores
with no atomic cross-db snapshot, the safe strategies are:

- **Stopped-copy (always safe, default).** Stop `sybil-api`, then copy the whole
  data dir. A stopped store has a settled redb fence and a matching qmdb slot.
- **Atomic filesystem snapshot (safe while running).** An LVM / ZFS / btrfs
  snapshot of `SYBIL_DATA_DIR` captures redb + qmdb at one instant, preserving
  the fence↔slot agreement. Snapshot first, then back up *from the snapshot*.
- **Plain online `cp` of a live store — UNSAFE.** It can capture a torn redb
  page, or a redb fence that points past what the qmdb copy captured. Such a
  backup may fail the recovery invariants and be unrestorable. `backup-store.sh`
  refuses this unless you explicitly assert safety.

---

## Taking a backup

```bash
# Stopped-copy (recommended). Stop sybil-api first, then:
SYBIL_DATA_DIR=/opt/sybil/data scripts/backup-store.sh --dest /opt/sybil/backups

# Equivalent, explicit:
scripts/backup-store.sh --data-dir /opt/sybil/data --dest /opt/sybil/backups --assume-stopped

# From an atomic FS snapshot while the service keeps running:
#   lvcreate -s ...  (or zfs snapshot / btrfs subvolume snapshot), mount it, then:
scripts/backup-store.sh --data-dir /mnt/snap/sybil/data --dest /opt/sybil/backups --allow-online
```

Behavior:

- If the source `sybil.redb` still has an open writer (detected via `fuser`,
  falling back to `lsof`), the script **refuses** unless `--assume-stopped` or
  `--allow-online` is given. If neither tool is installed it refuses and asks
  you to confirm with `--assume-stopped`.
- Output is a timestamped directory `…/sybil-store-<UTC>/` mirroring the data-dir
  layout (`sybil.redb` + `sybil.qmdb/`) plus a `BACKUP_MANIFEST.txt` recording
  source dir, timestamp, host, redb size, and the strategy used.
- The final stdout line is the backup directory path (script-friendly).

---

## Restore drill

`restore-store-drill.sh` proves a backup opens and serves, without touching the
live store or port:

```bash
scripts/restore-store-drill.sh /opt/sybil/backups/sybil-store-<UTC>
# optional: --api-binary /path/to/sybil-api   (skip the cargo build)
#           --timeout 60                       (health wait, default 30s)
```

The drill:

1. copies the backup into a throwaway `mktemp` dir,
2. builds (or uses a provided) `sybil-api`,
3. picks a free ephemeral port and starts the server with
   `SYBIL_DATA_DIR=<throwaway>`, `SYBIL_PORT=<ephemeral>`, `SYBIL_DEV_MODE=false`,
4. waits for `/v1/health`,
5. asserts `/v1/blocks/latest` returns a height and `/v1/state-root` returns a
   root from the restored state,
6. kills the server and removes the throwaway dir on exit.

Exit code is non-zero (and the server log tail is dumped) if the store fails to
open, the server crashes on boot, or health/blocks/state-root do not answer.

### Restoring for real

To restore into production, do the equivalent of the drill's copy step by hand,
against the *stopped* service:

```bash
systemctl stop sybil-api            # or: docker compose stop api
rm -rf /opt/sybil/data              # or move it aside
mkdir -p /opt/sybil/data
cp -a /opt/sybil/backups/sybil-store-<UTC>/. /opt/sybil/data/
rm -f /opt/sybil/data/BACKUP_MANIFEST.txt
systemctl start sybil-api
```

---

## What the drill does NOT prove

- **Currency at cutover.** The drill validates *this* backup, not that it is
  byte-current with production. Recovery replays only the WAL-protected work
  committed *after* the last block; anything lost before the backup instant is
  lost. Backup freshness is an operational (RPO) decision, not something the
  drill can attest.
- **Historical-serving completeness.** Pruned history (blocks, price points,
  candles) is not reconstructed; the drill only checks head-of-chain endpoints.
- **External side-effects.** L1 deposit/withdrawal re-indexing, Polymarket
  mirror re-sync, and any off-store JSON sidecars (`SYBIL_MARKET_REF_DATA_PATH`,
  `SYBIL_EVENT_SNAPSHOT_DIR`) are outside `SYBIL_DATA_DIR` and outside this drill.
- **Cross-binary compatibility.** The store carries a `store_layout_version`; a
  backup only restores under a binary with a compatible layout. Drill with the
  binary you intend to restore onto.
- **An unsafely-taken backup.** A plain online `cp` of a live store may *appear*
  to pass a drill and still be subtly inconsistent. Only stopped-copy or an
  atomic FS snapshot are trustworthy sources.
