# Runbook: sequencer store backup and restore

**Owning ticket:** SYB-223 item 2 · **Components:** `sybil-api`,
`matching-sequencer`, operations · **Scripts:** `scripts/store-backup.sh`,
`scripts/store-restore-drill.sh`

This runbook covers a short-freeze hot backup of the single-sequencer store,
an isolated restore drill, and an emergency production restore. A backup is not
accepted until the drill serves the exact state recorded in its manifest.

## Why the copy uses `docker pause`

The store is not one independently copyable redb file. `Store::open` in
`crates/matching-sequencer/src/store/commit.rs` opens `sybil.redb` and derives
the sibling `sybil.qmdb/` path with `path.with_extension("qmdb")`. Block commit
then:

1. writes the next account snapshot and typed state into the inactive qMDB
   slot;
2. checks that typed qMDB's root against the block header;
3. commits one redb transaction that stores the block and flips the
   authoritative qMDB fence.

Recovery reads only the slot named by the redb fence. An inactive qMDB slot
written before a crash is ignored if redb never committed the flip. These are
the explicit transaction and recovery invariants at the top of
`crates/matching-sequencer/src/store.rs` and in `save_block_inner`.

The pinned redb 4.0 crate is an ACID, crash-safe copy-on-write database, but its
public `Database` API has integrity checking and compaction—not an online
backup/snapshot API. Pausing only the simulation scheduler is also insufficient:
production disables `/v1/simulation/pause`, and acknowledged account/order/
control-plane requests can write redb between blocks.

`store-backup.sh` therefore freezes the **whole container** with `docker pause`,
copies the **whole `SYBIL_DATA_DIR`** while every userspace writer is frozen,
and unpauses it through an EXIT trap. This produces a stable crash image:
redb recovers its last ACID commit, and the redb fence selects the matching
qMDB slot. The API is briefly unresponsive during the copy, but the container
is not stopped or recreated.

After resuming the source, the script boots the source image against a second,
throwaway copy with a 24-hour block interval. It records the state that actually
restores—not a racy API sample taken before the freeze—in `manifest.json`.

## Take a production backup

Run on the deployment host from `/opt/sybil`:

```bash
cd /opt/sybil
install -d -m 0700 /opt/sybil/.tmp
TMPDIR=/opt/sybil/.tmp scripts/store-backup.sh --target prod
```

Production defaults are compose project `sybil`, container data directory
`/data`, and destination `/opt/sybil/backups`. Override the destination or
choose a known account sample when useful:

```bash
scripts/store-backup.sh --target prod \
  --dest /mnt/encrypted-backups/sybil \
  --account-id 42
```

The output directory is `sybil-store-<UTC>-<pid>/` and contains:

- `store/`: the complete copied data directory, including `sybil.redb` and
  `sybil.qmdb/`;
- `SHA256SUMS`: checksum for every copied file;
- `manifest.json`: source provenance, the consistency mechanism, exact block
  height/state root, and one complete account response.

The script fails and removes an incomplete output if the source cannot be
unpaused, the files are absent, the copied store cannot boot, or an account
sample cannot be read. The last stdout line is the completed backup path.

Do not use `docker cp` manually against a live, unpaused container. Do not copy
only `sybil.redb`; the fenced qMDB slots are required for recovery.

## Take a local itest backup

Target any running project created from `docker-compose.yml` plus
`docker-compose.itest.yml`:

```bash
scripts/store-backup.sh --target itest \
  --project sybil-itest-<id> \
  --dest ./store-backups
```

If the container is not compose-managed, use the explicit form:

```bash
scripts/store-backup.sh --target custom \
  --container <container-name-or-id> \
  --data-dir /itest-data \
  --dest ./store-backups
```

## Run the restore drill

Run from a checkout containing the binary version intended for restore:

```bash
TMPDIR=/home/anonymous/.cache/tmp \
  scripts/store-restore-drill.sh /opt/sybil/backups/sybil-store-<UTC>-<pid>
```

The drill validates checksums, creates a unique Compose project from the normal
base file plus the itest overlay, populates only that project's fresh
`itest-data` volume, builds/boots `sybil-api`, and tears the project down with
`down -v` on every exit. `--no-build` reuses an existing `sybil-api:itest`
image; `--port` and `--timeout` tune local execution.

“Restored OK” means all of the following are true:

- redb and the fenced qMDB slot pass startup recovery;
- `/v1/health` succeeds;
- `/v1/blocks/latest.height` exactly equals the manifest height;
- both `/v1/blocks/latest.state_root` and `/v1/state-root` exactly equal the
  manifest state root;
- `GET /v1/accounts/<sample>` is structurally equal to the complete account
  object recorded in the manifest.

Any difference exits nonzero. A 24-hour drill block interval prevents the
restored node from advancing before those exact comparisons.

## Restore to production

This is a destructive incident procedure. Keep the failed volume until the
replacement has passed verification.

1. Run `store-restore-drill.sh` against the candidate using the code/image
   revision you intend to deploy. Stop if it is not `restored OK`.
2. Take one final hot backup of the current production volume if the API still
   runs. Copy both candidate and safety backup off the host.
3. Stop every API writer, then the API:

   ```bash
   cd /opt/sybil
   docker compose -f docker-compose.yml -f docker-compose.prod.yml \
     stop sybil-polymarket sybil-arena sybil-prover-mock sybil-api
   ```

   Stop the separately managed L1 indexer too, if enabled.
4. Preserve the failed volume before changing it. The safest approach is a
   host/volume snapshot. If one is unavailable, copy `/data` from a one-shot
   container into a separately named archival volume.
5. Replace only the contents of `sybil-data` with the candidate:

   ```bash
   docker run --rm \
     -v sybil-data:/data \
     -v /opt/sybil/backups/sybil-store-<UTC>-<pid>/store:/restore:ro \
     --entrypoint sh sybil-api:latest -c \
     'find /data -mindepth 1 -maxdepth 1 -exec rm -rf {} + && cp -a /restore/. /data/'
   ```

6. Start `sybil-api` alone, watch its logs, then check health, state root, the
   manifest account, and that the head begins at or advances from the manifest
   height. Stop and preserve logs on any recovery/invariant error.
7. Restart the mirror, arena, mock prover, and L1 indexer. Run
   `scripts/post-deploy-smoke.sh` and the synthetic probe.

**Never run `just deploy-reset-state CONFIRM` during restore.** That recipe
removes `sybil-data` (along with several other state volumes) and creates a
fresh genesis. It is a consensus-redeploy tool, not a restart or restore step.

## Cadence and retention

For the private single-sequencer validium:

- take a backup at least daily and immediately before every deploy, schema
  change, or intentional state reset;
- drill the newest backup weekly and before relying on it for a change window;
- retain at least seven daily and four weekly generations;
- replicate encrypted copies off-host—backups only on the sequencer host do not
  protect against disk or host loss;
- monitor freeze duration and schedule large copies away from peak traffic.

The backup RPO is its creation time. The drill proves internal restorability
and exact recorded API state; it does not prove off-host replication, L1
side-effect replay, or compatibility with a different store layout version.
