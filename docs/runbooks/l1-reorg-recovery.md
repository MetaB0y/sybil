---
tags: [runbook, l1, bridge, recovery, security]
status: current
last_verified: 2026-07-15
---

# L1 source-integrity fail-stop and recovery

> **Executive summary:** a canonical block-hash mismatch, provider
> disagreement, finality regression, invalid block binding, or deposit-root
> mismatch is a bridge-integrity incident. The L1 indexer stops before
> accepting more input and writes a durable latch into its deployment-bound
> cursor. Freeze the API and both contracts, preserve the cursor and sequencer
> store, establish the canonical L1 view independently, and do not resume by
> editing or deleting the cursor. Sybil does not yet have a safe in-place
> inverse for an already-applied deposit or withdrawal event; that remains a
> real-funds deployment blocker.

## What the indexer guarantees

`SYBIL_L1_CURSOR_PATH` is required. Cursor schema v3 stores all of the following
as one synced-temp-file plus atomic-rename update:

- chain id and vault address;
- trust mode and sorted, non-secret provider identities;
- `next_from`, the first unprocessed L1 block;
- the number and canonical hash of `next_from - 1`;
- the number and hash of the last authenticated source tip, even when chunked
  scanning has not reached it; and
- an optional source-integrity incident latch.

Public/devnet operation uses `SYBIL_L1_TRUST_MODE=unanimous-finalized` with at
least two operator-asserted independently operated endpoints with distinct
URLs. The indexer takes
the lowest `finalized` height reported by all providers and requires every
provider to return the same hash for that height and every scanned ancestor.
Each vault-log query uses the agreed block hash (EIP-234). Each
`depositRootByCount(id)` call uses the deposit log's same block hash with
`requireCanonical=true` (EIP-1898). Security-relevant responses must be
identical; there is no fallback that drops an unavailable or disagreeing
provider.

The policy assumes at least one configured provider is honest and independently
operated. It detects a complete fabricated fork served by one provider because
the honest provider disagrees. It is not a consensus light client and cannot
detect every configured provider colluding on the same fabricated view.
Provider ownership, credentials, TLS, and configuration access are therefore
security controls, not availability-only details.

Before a poll calls the Sybil API, every provider must reproduce the persisted
checkpoint and previously authenticated source-tip hashes. Because an Ethereum
header commits its parent chain, this anchors the processed prefix. A later
source tip below the stored tip is a fatal finality regression even when the
scan checkpoint has not caught up. The range-tip hash is checked again after
ingestion. Changing the cursor's provider set or trust mode is refused.

A mismatch is fatal and writes `integrity_incident` to the cursor. The process
then keeps only `/metrics` and unhealthy `/healthz` alive; it performs no more
RPC or Sybil API calls. A restart refuses that latch without querying or
ingesting more L1 input. A temporarily unavailable provider may retry as a
whole-poll failure, but the indexer cannot advance the cursor, use only the
remaining endpoints, or submit bridge input.

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
   - chain id, vault address, trust mode, provider ids/owners/endpoints,
     confirmation settings (local mode only), and last known bridge status; and
   - receipts/logs/headers around the checkpoint from every configured provider.

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
- a fresh indexer cursor whose checkpoint and provider identity match the
  reviewed independent source set;
- successful deterministic bridge tests and an isolated store restore drill;
- API health, state-root, signed trading, deposit, withdrawal, and restart
  smoke; and
- explicit unpause order for the indexer, API writers, settlement, and vault.

## Confirmation and monitoring policy

- Local Anvil plumbing explicitly uses `unsafe-single-dev`. It may set
  `confirmations=0` and `min_confirmations=0` only when no real value or public
  finality claim is involved; startup logs a `DEV-ONLY` warning.
- Public/devnet operation uses `unanimous-finalized`, at least two independently
  operated providers, and unique non-secret ids. Block-count confirmation
  settings are ignored in this mode.
- The finalized provider policy is implemented and adversarially tested under
  its at-least-one-honest-provider assumption. Real funds remain blocked on the
  complete recovery mechanism above and the other incomplete proof/DA/
  governance production gates; fail-stop detection is not recovery.
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
| `L1IndexerFatalFailure` | Metrics-only fail-stop; `kind` includes provider disagreement, invalid authenticated view, finality regression, canonical hash, deposit root, cursor, configuration, or persistence failure | Capture `/healthz`, the fatal series, cursor, and logs; follow Immediate containment |
| `L1IndexerNotReady` | Listener is reachable but ingestion stayed unready for one minute | Correlate the fatal counter and container health; do not restart-loop a latched incident |
| `L1IndexerRpcFailureBurst` | Three consecutive whole-quorum polls failed; success resets the streak | Check every configured provider; never remove the failing endpoint as an unreviewed fallback |
| `L1IndexerConfirmedLagHigh` | Durable checkpoint stayed over 64 authenticated source-prefix blocks behind for ten minutes | Inspect provider latency/errors, API failures, cursor persistence failures, and checkpoint progress |

`sybil_l1_indexer_fatal_failures_total` is intentionally an absolute
process-local counter. A cold-start cursor failure first appears at `1`, so the
alert does not use `increase()` and fires on the first scrape. A clean process
with no incident resets it to zero/absence; a latched cursor recreates the page
on every restart. `sybil_l1_indexer_integrity_latched` is an additional
dashboard signal. `sybil_l1_indexer_source_policy` and
`sybil_l1_indexer_provider_count` expose the active boundary, while `ready=0`
makes `/healthz` return 503.

No cursor file is valid only for a deliberate first bootstrap from the
configured start block. Once a deployment has produced a cursor, a missing
mounted file or volume is a storage incident: stop the indexer and restore the
preserved cursor instead of accepting a rescan. An existing JSON file with a
missing checkpoint, source identity, or other required v3 field fails startup as
`kind="cursor_invalid"` and pages on its first scrape.

For a cursor persistence failure, preserve both `cursor.json` and any
`cursor.json.tmp` before inspecting the filesystem, mount, free space, and
permissions. For RPC bursts, never switch to a subset that happens to answer or
change the provider identities on the existing cursor. Provider inconsistency
is itself a durable integrity incident.
