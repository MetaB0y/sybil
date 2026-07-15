---
tags: [runbook, l1, bridge, recovery, security]
status: current
last_verified: 2026-07-15
---

# L1 deep-reorg fail-stop and recovery

> **Executive summary:** a canonical block-hash mismatch is a bridge-integrity
> incident. The L1 indexer stops before accepting more logs and writes a durable
> latch into its deployment-bound cursor. Freeze the API and both contracts,
> preserve the cursor and sequencer store, establish the common L1 ancestor
> with independent providers, and do not resume by editing or deleting the
> cursor. Sybil does not yet have a safe in-place inverse for an already-applied
> deposit or withdrawal event; that is a real-funds deployment blocker.

## What the indexer guarantees

`SYBIL_L1_CURSOR_PATH` is required. Cursor schema v2 stores all of the following
as one synced-temp-file plus atomic-rename update:

- chain id and vault address;
- `next_from`, the first unprocessed L1 block;
- the number and canonical hash of `next_from - 1`; and
- an optional deep-reorg incident latch.

Before a poll reads new logs or calls the Sybil API, the indexer asks the RPC
for the persisted checkpoint block and requires its hash to match. Because an
Ethereum header commits its parent chain, this anchors the whole processed
prefix. The indexer also compares every returned deposit/withdrawal log's block
hash with the canonical header and requires the range-tip hash to stay stable
across ingestion. Deposit roots retain their separate
`depositRootByCount(id)` reconciliation.

A mismatch is fatal and writes `reorg_incident` to the cursor. The process then
keeps only `/metrics` and unhealthy `/healthz` alive; it performs no more RPC or
Sybil API calls. A restart refuses that latch without querying or ingesting
more L1 input. Missing or temporarily unavailable RPC results may retry, but
still cannot advance the cursor or submit bridge input.

Legacy cursor-only JSON has no proof of which chain produced already-credited
events and is rejected. Do not manufacture a checkpoint for it from today's
RPC response: that would bless the current fork without proving it matches the
fork already applied by the sequencer.

## Immediate containment

1. Do not restart or kill the metrics-only indexer until its metrics and
   `/healthz` body are captured. Stop any external supervisor that restarts
   unhealthy containers. Preserve the complete fatal log, especially
   `context`, `block_number`, `expected`, and `observed`.
2. Stop API order intake and every automated API writer, then stop
   `sybil-api`. A sidecar exit alone does not freeze trading against a possibly
   unbacked balance or stale withdrawal lifecycle.
3. Use the contract administrator's immediate pause authority on both
   `SybilSettlement` and `SybilVault`; follow the
   [administrator runbook](admin-keys.md). Record the transactions and read the
   paused state back. Escape claims intentionally bypass pause.
4. Snapshot and copy off-host, without modification:

   - the latched cursor file and its filesystem metadata;
   - the complete sequencer data directory (`sybil.redb` and `sybil.qmdb/`);
   - indexer/API logs and exact image or jj revision;
   - chain id, vault address, RPC endpoints, confirmation settings, and last
     known bridge status; and
   - receipts/logs/headers around the checkpoint from the configured RPC.

Never delete the cursor, change its expected hash, advance `next_from`, replay
only a convenient prefix, directly debit/credit an account, or mark a replaced
withdrawal as benign. Those actions sever the audit trail between L1 custody
and acknowledged validium state.

## Establish the affected interval

Use at least two independently operated RPC providers plus an explorer/archive
source where available:

1. Compare the expected and observed checkpoint headers and walk backward to
   the last common ancestor.
2. Enumerate every vault deposit and withdrawal queued/finalized/cancelled log
   from the common ancestor through `next_from - 1` on both forks.
3. Compare deposit ids/roots, nullifiers, transaction receipts, and canonical
   block hashes with the sequencer's committed bridge cursor/root, withdrawal
   leaves/statuses, and observed L1 height.
4. Determine whether the signal is a genuine canonical reorg, a faulty RPC, or
   inconsistent provider data. A provider fault still requires explicit
   incident closure; do not clear the latch merely because the original
   endpoint later returns the expected hash.

## Recovery decision

There is no supported in-place rollback for an already-credited deposit or an
already-applied withdrawal lifecycle event. Those transitions may have affected
later trades, reservations, roots, proofs, and contract actions. Rewriting the
bridge row alone would create a different invalid state.

- **Local/devnet with no real value:** preserve the incident state, then use the
  reviewed [fresh-genesis procedure](fresh-genesis-redeploy.md) or an explicitly
  reviewed complete-state reconstruction. Start a new cursor file only for the
  new deployment/state domain.
- **Real value or any disputed custody:** remain paused. Recovery requires a
  reviewed mechanism that reconstructs the complete validium state at a safe
  ancestor and deterministically replays both off-chain activity and canonical
  L1 inputs, or an equivalent validity/governance procedure. That mechanism is
  not implemented; Sybil is not ready to bridge real funds across this event.

Archive the old latched cursor permanently. A replacement cursor is an output
of the reviewed recovery/new-deployment plan, never an operator shortcut.

## Resume gate

Resume only after all of these are recorded and independently reviewed:

- root cause and common ancestor;
- treatment of every affected deposit and withdrawal event;
- corrected sequencer state/chain identity and matching vault deposit
  checkpoint;
- a fresh indexer cursor whose checkpoint matches two independent RPC views;
- successful deterministic bridge tests and an isolated store restore drill;
- API health, state-root, signed trading, deposit, withdrawal, and restart
  smoke; and
- explicit unpause order for the indexer, API writers, settlement, and vault.

## Confirmation and monitoring policy

- Local Anvil plumbing may use `confirmations=0` and `min_confirmations=0` only
  when no real value or public finality claim is involved.
- Public-chain dev/test operation uses at least 64 confirmations and sets
  `SYBIL_L1_MIN_CONFIRMATIONS` to the same floor. The hash checkpoint is still
  mandatory.
- Confirmation count and one JSON-RPC provider are not a production finality
  proof. Before real funds, adopt and test an explicit finalized-tag/provider
  quorum or receipt-proof policy and implement the complete recovery mechanism
  above.
- Enable the opt-in Compose `l1-indexer` profile only after configuring its
  vault and RPC. The dedicated cursor volume is deployment identity; never
  reuse it for another chain or vault.
- VictoriaMetrics scrapes the independent listener on port 9102. Fatal startup
  and runtime failures remain scrapeable even when ingestion is halted.
- Because the Compose profile is optional, the shared rules do not page merely
  because its scrape target is absent. A deployment that enables L1 ingestion
  must also make its process supervisor or target-absence monitor page when the
  container disappears. vmalert owns semantic pages from the metrics below;
  logs remain incident evidence, not the only alert transport.

## Indexer alerts and diagnosis

The L1/indexer on-call owns these alerts. A critical page freezes bridge-risking
writes first; it is not an instruction to delete a cursor or try another RPC.

| Alert | Meaning | First checks |
|---|---|---|
| `L1IndexerFatalFailure` | Metrics-only fail-stop; `kind` names canonical hash, deposit root, cursor, configuration, or service failure | Capture `/healthz`, the fatal series, cursor, and logs; follow Immediate containment |
| `L1IndexerNotReady` | Listener is reachable but ingestion stayed unready for one minute | Correlate the fatal counter and container health; do not restart-loop a latched incident |
| `L1IndexerRpcFailureBurst` | Three consecutive polls failed; success resets the streak | Compare the configured provider with independent sources and check for finality disagreement before changing endpoints |
| `L1IndexerConfirmedLagHigh` | Durable checkpoint stayed over 64 confirmed blocks behind for ten minutes | Inspect RPC latency/errors, Sybil API failures, cursor persistence failures, and checkpoint progress |

`sybil_l1_indexer_fatal_failures_total` is intentionally an absolute
process-local counter. A cold-start cursor failure first appears at `1`, so the
alert does not use `increase()` and fires on the first scrape. A clean process
with no incident resets it to zero/absence; a latched cursor recreates the page
on every restart. `sybil_l1_indexer_reorg_latched` is an additional dashboard
signal, while `ready=0` makes `/healthz` return 503.

No cursor file is valid only for a deliberate first bootstrap from the
configured start block. Once a deployment has produced a cursor, a missing
mounted file or volume is a storage incident: stop the indexer and restore the
preserved cursor instead of accepting a rescan. An existing JSON file with a
missing checkpoint or other required v2 field fails startup as
`kind="cursor_invalid"` and pages on its first scrape.

For a cursor persistence failure, preserve both `cursor.json` and any
`cursor.json.tmp` before inspecting the filesystem, mount, free space, and
permissions. For RPC bursts, never bypass confirmation depth or switch to an
agreeable endpoint without recording the comparison; provider inconsistency is
itself an integrity incident.
