# Prover daemon operations

The production-shaped prover is `sybil-prover daemon`. It is one standalone
service with a redb authority, immutable proof artifacts, an authenticated
sequencer-outbox client, deterministic multi-block epoch assembly, and one
proof subprocess at a time. The older `worker` and `serve` commands remain
debugging tools; they do not provide the daemon's recovery guarantees.

## Modes

- `mock` executes the full native epoch verifier and emits a deterministic,
  domain-separated `ProofKind::Mock` envelope. Base Compose uses this for cheap
  end-to-end transport and restart testing.
- `stark` is the real milestone mode. It encodes the streamed epoch input,
  runs `cargo openvm prove app`, locally runs `cargo openvm verify app`, and
  publishes `ProofKind::OpenVmStark` only after both succeed.
- `evm` is deliberately disabled. GitHub issue #13 owns Halo2/EVM resources,
  verification, and submission. Neither mock nor STARK envelopes can enter the
  L1 calldata path.

The repository default is `stark`; Compose explicitly selects `mock` because
the small runtime image and current 2 GB devnet host do not contain or have
capacity for the pinned OpenVM toolchain. Run STARK mode from a repository
checkout on measured prover hardware.

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
| `--memory-limit-mib` | Linux `RLIMIT_AS` ceiling; zero disables it. This limits virtual address space, not RSS |
| `--command-timeout-secs` | per encoder/prove/verify subprocess timeout |

Start at 1–4 blocks for real STARK measurements. Increase the target only when
measured proof throughput stays ahead of block ingress and peak RSS/disk have
headroom. A new value affects only unassembled work.

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

After restore, start the daemon and require `/readyz`, then inspect:

```bash
curl -fsS http://127.0.0.1:3002/v1/status | jq
curl -fsS http://127.0.0.1:3002/metrics | grep '^sybil_prover_'
```

Do not edit artifact directories or redb records manually. Quarantined outputs
are evidence for debugging; remove them only after the corresponding epoch is
proven and retained according to policy.
