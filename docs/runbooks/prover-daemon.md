# Prover daemon operations

The production-shaped prover is `sybil-prover daemon`. It is one standalone
service with a redb authority, immutable proof artifacts, an authenticated
sequencer-outbox client, deterministic multi-block epoch assembly, and one
proof subprocess at a time. The older `worker` and `serve` commands remain
debugging tools; they do not provide the daemon's recovery guarantees.

## Modes

- `mock` executes the full native epoch verifier and emits a deterministic,
  domain-separated `ProofKind::Mock` envelope. The explicit Compose `validity`
  profile uses this for bounded end-to-end transport and restart testing.
- `stark` is the real milestone mode. It encodes the streamed epoch input,
  runs `cargo openvm prove app`, locally runs `cargo openvm verify app`, and
  publishes `ProofKind::OpenVmStark` only after both succeed.
- `evm` is deliberately disabled. GitHub issue #13 owns Halo2/EVM resources,
  verification, and submission. Neither mock nor STARK envelopes can enter the
  L1 calldata path.

The repository default is `stark`; Compose explicitly selects `mock` because
the small runtime image does not contain the pinned OpenVM toolchain. The
current 2 GB product devnet does not enable that profile: the mock daemon's
retained job stock exhausted its cgroup during a live soak (#137). Run bounded
mock integration tests locally and STARK mode from a repository checkout on
measured prover hardware.

Artifact retention is chain identity. On the production host, the product
overlay disables source proof jobs and DA payloads; adding only
`--profile validity` to that running chain cannot create the missing historical
sequence. `just deploy-prover-daemon CONFIRM` applies the validity overlay,
clears all coupled state, and starts from fresh genesis. The API also persists
the selected mode and refuses an in-place switch.

## Start locally

The sequencer API must use persistent storage and expose its service-gated
outbox. Use the same service bearer for source pull/ack:

```bash
export SYBIL_SERVICE_TOKEN='<service token>'
just prover-daemon-mock "$SYBIL_SERVICE_TOKEN"
# or, on a prover host with the pinned OpenVM v2 toolchain:
just prover-daemon-stark "$SYBIL_SERVICE_TOKEN"
```

Important configuration:

| Option / environment | Meaning |
|---|---|
| `--db` / `SYBIL_PROVER_DB` | redb authority; back it up with the artifacts |
| `--artifacts-dir` | immutable envelopes and proof payloads |
| `SYBIL_PROVER_PROOF_KIND` | `mock`, `stark`, or fail-closed `evm` |
| `SYBIL_PROVER_EPOCH_BLOCKS` | target for future epochs; existing epochs never reshape |
| `SYBIL_PROVER_SOURCE_URL` | Sybil API base URL |
| `SYBIL_PROVER_SOURCE_TOKEN` | API service bearer for pull/ack |
| `SYBIL_PROVER_AUTH_TOKEN` | separate bearer for daemon ingest/admin mutations |
| `SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS` | sequencer-side source safety window after exact-byte ack |
| `SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS` | independent cadence for bounded source pruning |
| `SYBIL_ACKNOWLEDGED_PROOF_JOB_MAX_ROWS_PER_PASS` | maximum old source rows examined in one pass |
| `--memory-limit-mib` | Linux `RLIMIT_AS` ceiling; zero disables it. This limits virtual address space, not RSS |
| `--command-timeout-secs` | per encoder/prove/verify subprocess timeout |

Start at 1–4 blocks for real STARK measurements. Increase the target only when
measured proof throughput stays ahead of block ingress and peak RSS/disk have
headroom. A new value affects only unassembled work.

The bounded Compose acceptance soak runs only a persistent local sequencer and
the mock daemon, proves several four-block epochs, hard-kills the prover while
block production continues, restarts it from redb, and asserts a contiguous
proven prefix:

```bash
just prover-compose-soak
```

It uses an isolated project and bind-mounted directories under `target/`; it
does not touch the normal Compose volumes. This validates orchestration and
recovery, not STARK performance.

Do not use `--memory-limit-mib` as the primary production RSS control. OpenVM
reserves substantially more virtual address space than its resident set, so an
apparently generous `RLIMIT_AS` can abort a proof well below the host's real
memory limit. Run the daemon in a dedicated container/systemd cgroup with an
actual-memory ceiling and a higher OOM-kill preference than the shell or
supervisor. Keep host swap and several GiB of non-prover headroom. A July 2026
one-block OpenVM v2 acceptance run completed with a 22.0 GiB charged-memory
peak under a 24 GiB hard cap. The same run was killed at a 22 GiB hard cap on a
32 GiB no-swap desktop; after adding 16 GiB host swap it completed while using
less than 400 MiB of swap. Treat those figures as a floor, not a capacity SLO.

## Health and status

- `GET /healthz`: process liveness; a slow proof does not fail liveness.
- `GET /readyz`: startup reconciliation and database readiness.
- `GET /v1/status`: durable frontiers, queue sizes, backend, and owner UUID.
- `GET /v1/epochs` and `/v1/epochs/{first_block_height}`: typed epoch states,
  attempts, errors, and artifact envelope.
- `GET /metrics`: frontiers, proof/source counters, queue bytes, state counts,
  lease recovery, and compatibility metrics used by existing dashboards.
- `GET /proofs/latest`: read-only compatibility projection for synthetic
  monitoring. It is not persistence authority.

The scheduler, source puller, and HTTP server are process-critical. If any one
panics, returns an error, or returns cleanly before shutdown, the daemon marks
itself unready, stops its siblings, drains HTTP, and exits nonzero. Compose's
bounded `on-failure` policy can therefore restart a transient process failure;
an unhealthy-but-live shell is not used as a substitute for supervision.

Authenticated mutations:

```bash
curl -X POST -H "Authorization: Bearer $SYBIL_PROVER_AUTH_TOKEN" \
  http://127.0.0.1:3002/v1/admin/seal

curl -X POST -H "Authorization: Bearer $SYBIL_PROVER_AUTH_TOKEN" \
  http://127.0.0.1:3002/v1/admin/retry/1
```

Partial sealing is for deploy/genesis boundaries, not ordinary scheduling.
Manual retry is audited and moves an exhausted/permanent epoch to `Ready`; it
preserves the monotonic attempt number and cannot rewrite a proven or currently
leased epoch.

## Crash and recovery behavior

On restart the daemon:

1. quarantines interrupted temporary directories;
2. makes expired attempts retryable when automatic attempts remain, otherwise
   marks the epoch permanently failed;
3. validates every redb-referenced artifact;
4. returns missing/corrupt artifacts to retry; and
5. adopts an exact valid final artifact left by a crash after atomic rename but
   before the redb commit.

An outbox response is acknowledged only after its exact bytes and digest commit
to prover redb. A lost ack repeats the same source row and is idempotent. A
conflicting height, gap, invalid witness, mismatched epoch output, or invalid
source metadata fails closed. The first non-proven epoch is always the proving
barrier, so a later range cannot skip a poisoned transition.

### Source retention and ownership

After acknowledgement, the sequencer retains the source job only for its
configured safety window. It then removes the matching job/ack pair atomically;
unacknowledged or digest-mismatched rows survive. Maintenance uses a durable
rotating cursor and a hard rows-examined budget, so an old unacknowledged prefix
does not monopolize redb. Treat the acknowledgement as an ownership transfer:
after the window, deleting or losing prover redb cannot be repaired from the
sequencer's canonical block or DA rows.

The API exports `sybil_acknowledged_proof_jobs_pruned_total`,
`sybil_acknowledged_proof_job_rows_examined_total`,
`sybil_proof_job_outbox_oldest_retained_height`, and
`sybil_acknowledged_proof_job_maintenance_failures_total`. The last counter
drives `ProofJobRetentionMaintenanceFailed` as an absolute first-scrape-safe
critical page. On alert, do not delete either table or relax digest checks:
preserve the sequencer store, compare the prover's ingested digest/height, and
repair the storage or ownership disagreement before allowing pruning to resume.

Proof attempts carry an owner UUID, attempt number, and renewable deadline.
Subprocess/resource failures retry with bounded exponential backoff and
deterministic jitter. Validity failures halt permanently until explicit
operator review. Graceful shutdown drops the kill-on-drop OpenVM child and
leaves its durable lease for normal recovery. The scheduler reconciles expired
leases both at startup and on every idle tick, so a worker disappearing after
startup cannot leave the frontier indefinitely stuck in `Proving`.

## Backup and restore

Back up the redb file and artifact tree as one logical set while the daemon is
stopped. The database contains state/ownership/digests; the files contain large
immutable payloads. Restoring only one side intentionally triggers
reconciliation and may require re-proving.

Keep at least one verified backup generation older than the sequencer's source
safety window. Before shortening that window, restore the prover backup in an
isolated directory and confirm its durable ingested height covers the jobs that
the sequencer will make eligible for pruning.

After restore, start the daemon and require `/readyz`, then inspect:

```bash
curl -fsS http://127.0.0.1:3002/v1/status | jq
curl -fsS http://127.0.0.1:3002/metrics | grep '^sybil_prover_'
```

Do not edit artifact directories or redb records manually. Quarantined outputs
are evidence for debugging; remove them only after the corresponding epoch is
proven and retained according to policy.
