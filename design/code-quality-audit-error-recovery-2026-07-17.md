---
tags: [audit, code-quality, rust, errors, recovery, durability, operations]
layer: cross-cutting
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Error, panic, retry, and durable-recovery audit

Date: 2026-07-17  
Cluster: production Rust failure identity, retry classification, partial
durable writes, crash recovery, and intentionally fatal invariants  
Primary technique: error-path tracing plus durable-operation and retry matrices

## Verdict

Sybil's consensus-critical recovery core is substantially stronger than its
service-edge recovery code. The sequencer commits through an atomic redb fence
and exact WAL interval, history advances every projection and its checkpoint in
one transaction, the prover publishes validated artifacts before committing
their durable row, and the L1 indexer both fsyncs its cursor and latches
integrity failures.

The audit found seven concrete service-edge defects and fixed the bounded
ones:

- a transient API or network failure while reattaching a persisted market-maker
  account was treated as proof the account did not exist, causing a newly
  funded account to be minted;
- Polymarket market creation had no idempotency key, and market/group mappings
  were checkpointed only at the end of a long synchronization cycle;
- the Polymarket mapping and raw-event snapshot paths described their writes as
  durable but did not sync both the published file and its parent directory;
- an unavailable sequencer actor or persistence layer was flattened to HTTP
  500 instead of a retryable, stable 503 contract;
- the history process swallowed signal-registration/stream failures and could
  exit successfully without supervising its projector;
- recoverable API startup failures used `panic!`, `expect`, or `unwrap` rather
  than returning contextual process errors; and
- the API tolerated failure to create a configured durable event-snapshot
  directory, silently starting without the requested persistence boundary.

The remaining high-risk gap cannot be closed locally: funded service-account
creation still lacks an idempotent caller intent, so a lost response after
commit can orphan a live funded account. It is tracked in
[GitHub #188](https://github.com/MetaB0y/sybil/issues/188). The remaining
cross-service local-file implementations need one crash-safe primitive and
fault matrix; that work is
[GitHub #189](https://github.com/MetaB0y/sybil/issues/189). Protocol-level
market-group creation identity was already
[GitHub #129](https://github.com/MetaB0y/sybil/issues/129); the audit added the
Polymarket crash witness to that issue and implemented a bounded adoption path.

## Evidence boundary

The review covered production failure and durable-write paths in:

- `matching-sequencer`, including store fencing, acknowledged-write WAL replay,
  actor loss, persistence errors, and recovery tests;
- `sybil-api`, including startup, signal handling, sequencer-to-HTTP error
  conversion, event snapshots, off-block metadata, and worker ownership;
- `sybil-history`, including projector transactions, duplicate/gap handling,
  HTTP/signal supervision, and process exit;
- `sybil-prover`, including job leases, artifact publication, database commit,
  retry scheduling, and reopen adoption;
- `sybil-l1-indexer`, including cursor publication, fatal latches, and transient
  versus integrity failures;
- `sybil-polymarket`, `sybil-native`, `sybil-market-maker`, and `sybil-client`,
  including remote-create identity, local checkpoints, account reattachment,
  retryable reads, and ambiguous writes; and
- `sybil-oracle`, `sybil-custody`, `sybil-verifier`, and `sybil-zk` for panic
  and fail-stop boundary classification.

Tests, generated code, archived research, benches, and fuzz harnesses were
excluded from the production panic inventory except when they supplied recovery
evidence. The review ran local process/package tests and inspected existing
fault harnesses. It did not cut power during a real filesystem write, corrupt a
production volume, call a live provider, submit an L1 transaction, run a real
OpenVM proof, or deploy.

## Architecture context read

The review used the root instructions and applicable crate instructions for the
sequencer, API, history, prover, L1 indexer, custody, oracle, client, native and
Polymarket integrations, market maker, verifier, and ZK crates. The current
architecture and runbooks read included:

- `Persistence`
- `Acknowledged-Write WAL Replay`
- `Block Lifecycle`
- `Historical Data Serving`
- `Data Availability`
- `ZK Integration Path`
- `L1 Settlement and Vault`
- `Operator Replacement`
- `Deployment Profiles`
- the prover and L1 operational runbooks

The controlling invariants were:

- only landed integer state behind the verifier boundary is protocol truth;
- an acknowledged canonical write must survive restart or recovery must fail
  closed;
- retry is safe only when the operation is read-only or has a stable
  idempotency/receipt identity;
- a partial durable publication may expose either the complete old value or the
  complete new value, never malformed intermediate state;
- integrity/validity failures remain latched and actionable; and
- availability failures preserve enough structured identity for clients and
  supervisors to retry without leaking internal storage details.

## Research basis and method

- The Rust Book distinguishes
  [recoverable `Result` errors from unrecoverable panics](https://doc.rust-lang.org/stable/book/ch09-00-error-handling.html)
  and recommends `expect` only where the programmer can explain why failure is
  impossible. The inventory therefore did not mechanically replace every
  `expect`; it classified pre-proved local invariants separately from
  environment, transport, configuration, and durable-state failures.
- The Rust Reference defines
  [panic as unwinding or aborting the current computation](https://doc.rust-lang.org/stable/reference/panic.html).
  A task panic was therefore traced through its `JoinError` and process owner,
  rather than treated as a local log event.
- Tokio's
  [`JoinError`](https://docs.rs/tokio/latest/tokio/task/struct.JoinError.html)
  preserves cancellation and panic identity. This supported the prior
  supervisor work and the present distinction between a failed child and an
  ordinary service error.
- [`anyhow::Context`](https://docs.rs/anyhow/latest/anyhow/trait.Context.html)
  preserves the underlying typed cause while adding operation/resource
  context. Startup fixes follow the same principle with path- and
  operation-specific messages rather than `unwrap`.
- AWS's guidance on
  [safe retries with idempotent APIs](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/)
  motivated the explicit separation of read retries, caller-stable creation
  keys, and writes whose response can be lost after commit.
- redb documents automatic
  [crash recovery on database open](https://docs.rs/redb/latest/redb/struct.Database.html)
  and an explicit
  [`WriteTransaction::commit`](https://docs.rs/redb/latest/redb/struct.WriteTransaction.html).
  The review checked the repository's transaction boundaries instead of
  inferring durability from the database choice alone.
- Clippy's
  [restriction-lint documentation](https://doc.rust-lang.org/stable/clippy/index.html)
  treats `unwrap_used`, `expect_used`, and `panic` as opt-in review tools, not a
  universal default. They were run diagnostically and every warning class was
  interpreted in repository context.

The audit procedure was:

1. Run targeted `unwrap_used`, `expect_used`, `panic`, `todo`, and
   `unimplemented` diagnostics across production service targets.
2. Trace environment/configuration, actor, persistence, transport, integrity,
   and task-exit errors to their process/API boundary.
3. Enumerate multi-step durable operations and locate their transaction,
   temporary-file, sync, rename, database-commit, and reopen boundaries.
4. For every retry, ask whether the call is read-only, keyed idempotently,
   durably receipted, or ambiguous after transport loss.
5. Check that permanent integrity failures latch and retryable availability
   failures remain structured without exposing secret paths or provider data.
6. Fix local defects, add narrow regression assertions, and file architectural
   work where a local retry or timeout would worsen ambiguity.

## Panic and error inventory

The diagnostic command emitted 139 warning instances across the selected
workspace targets and their local workspace dependencies:

```text
cargo clippy \
  -p matching-sequencer -p sybil-api -p sybil-history -p sybil-prover \
  -p sybil-l1-indexer -p sybil-market-maker -p sybil-native \
  -p sybil-polymarket -p sybil-oracle -p sybil-custody \
  -p sybil-verifier -p sybil-zk --lib --bins -- \
  -W clippy::unwrap_used -W clippy::expect_used -W clippy::panic \
  -W clippy::todo -W clippy::unimplemented
```

That number is an inventory, not a defect count or quality score. The dominant
class was a proved local invariant in verifier/sequencer code: fixed-width
hashes, non-empty structures established immediately above the call, or
post-validation enum/state relationships. Those sites are appropriate
fail-stop boundaries unless a future refactor invalidates their proof.

The actionable classes were environment-dependent startup assumptions,
ignored signal errors, generic HTTP flattening, and crash windows between a
remote side effect and its local checkpoint. No production `todo!` or
`unimplemented!` path was accepted as a hidden runtime fallback in the scoped
services. The queued static-lint cluster should turn this reviewed distinction
into scoped policy and an explicit allowlist, not enable restriction lints
workspace-wide without context.

## Durable-operation matrix

| Operation | Publication / recovery boundary | Failure behavior | Audit result |
|---|---|---|---|
| Sequencer block and acknowledged writes | Write inactive qMDB state, then atomically commit the redb fence and exact WAL interval | Reopen follows the fence; invalid interval/recovery fails stop; crash harness covers commit points | Strong, accepted |
| History raw batch + projections + candles + checkpoint | One redb write transaction | Exact duplicate is a verified no-op; gap/conflict fails closed | Strong, accepted |
| Prover artifact + proof row | Build attempt directory, sync/validate/hash, atomic publish, then redb commit | Reopen adopts a valid published artifact after crash between rename and DB commit; leases/retries are durable | Strong, accepted |
| L1 cursor + fatal integrity latch | Temp write, file sync, atomic rename, parent-directory sync | Identity mismatch or integrity failure remains latched; transient providers retry separately | Strong, accepted |
| Polymarket mapping | Same-directory temp, file sync, rename, parent sync; save immediately after each remote create/extend | Reopen sees complete old/new mapping; remote market retries use a deterministic creation key | Fixed |
| API raw event snapshot | Unique same-directory temp, file sync, rename, parent sync in blocking worker | Concurrent/stale temp names do not overwrite each other; configured directory failure blocks startup | Fixed |
| Native deployment/MM state | Temp + rename, but no file/directory sync and a fixed temp name | Atomic-reader intent exists, but power-loss and concurrent/stale-temp behavior are not proved | Open in #189 |
| API off-block market metadata | Direct overwrite; warning-only failure | Rebuildable from mirror, but a crash can leave malformed JSON and the documented durability is weaker than implied | Open in #189 |
| Integration signer creation | Direct create/write with path-specific variations | Production can pre-provision, but locally retained secret publication and permissions are inconsistent | Open in #189 |

## Retry and failure-classification matrix

| Failure class | Required behavior | Observed disposition |
|---|---|---|
| Read-only transport/HTTP failure | Bounded retry/backoff or fail current cycle; never mutate identity | Existing clients generally comply; MM reattachment now fails closed on non-404 |
| Authoritative account 404 | Persisted identity is absent on current server; replacement may be provisioned | Explicitly classified in native and Polymarket MM |
| Account create response lost after commit | Retry only with durable caller intent/receipt | Open high-priority gap #188 |
| Polymarket market create response lost | Retry with stable provider-derived creation key | Fixed with domain-separated BLAKE3 key |
| Market-group create response/checkpoint lost | Adopt unique compatible group locally; long-term protocol key | Bounded recovery fixed; canonical identity remains #129 |
| Sequencer actor unavailable | Stable retryable service response | Fixed: `503 SEQUENCER_UNAVAILABLE` |
| Sequencer persistence unavailable | Log internal identity; return non-leaking stable retryable response | Fixed: `503 SEQUENCER_PERSISTENCE_UNAVAILABLE` |
| Invalid recovery / integrity mismatch | Never retry as availability; latch or exit | Existing sequencer/prover/L1 paths accepted |
| Best-effort post-commit DA publication | May fail without changing validity; retain explicit observability | Existing documented behavior accepted |

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| ER-1 | High | Native and Polymarket MM reattachment treated every `get_account` error as account absence, so a transient 503/network error could mint another funded identity. Native also treated a corrupt local state file as absence. | Fixed: only authoritative 404 permits replacement; transient/corrupt state fails startup. |
| ER-2 | High | Polymarket markets had no caller identity and their mappings were saved only after later metadata/group I/O, creating duplicate allocation windows after crash or lost response. | Fixed with deterministic creation keys and immediate durable checkpoints. |
| ER-3 | High | Market-group create has no protocol idempotency key; a crash before local checkpoint could wedge recovery on an already-grouped market. | Bounded list/adopt recovery added; durable protocol identity remains #129. |
| ER-4 | Medium | Mapping and raw-event snapshot rename paths omitted file/parent sync while documentation called them durable. Fixed temp names also risked stale-file collisions. | Fixed with synced publication and unique snapshot temp names. Broader consolidation is #189. |
| ER-5 | Medium | `SequencerError::ActorGone` and `Persistence` became generic HTTP 500, hiding retryability; persistence prose could expose internals if forwarded. | Fixed with two stable 503 codes, internal logging, and non-leaking response tests. |
| ER-6 | Medium | History ignored signal setup/stream errors and did not treat unexpected HTTP completion as process failure. An early-error branch initially bypassed projector stop and was caught by compilation/review before completion. | Fixed with fallible signal future, pinned server supervision, and projector stop before result propagation. |
| ER-7 | Medium | API telemetry, data-directory setup, oracle bootstrap, read-model initialization, listener, and server failures used panic/unwrap or lacked operation context. Configured event-snapshot directory failure only warned. | Fixed as contextual returned startup errors; configured persistence now fails closed. |
| ER-8 | High | Funded account create still has an unavoidable response/checkpoint ambiguity even after ER-1. | Open as #188, Project 1 Todo/Backlog/High. |
| ER-9 | Medium | Restart-durable operational files have fragmented atomicity, fsync, permissions, stale-temp, and fault-test behavior. | Open as #189, Project 1 Todo/Backlog/Medium. |

## Implemented changes

- Added domain-separated, normalized BLAKE3 Polymarket market creation keys
  within the API's 128-character key contract.
- Checkpointed each created market, created/adopted group, group extension, and
  event-sync marker immediately instead of waiting for the complete mirror
  cycle.
- Reconciled a missing local NegRisk checkpoint by adopting a uniquely
  compatible server group before attempting allocation.
- Made mapping publication sync the temporary file, atomically replace the
  target, and sync its parent directory.
- Made event-snapshot publication run off the async executor, use a unique
  timestamp/PID/nonce temporary name, sync contents, rename, sync the parent,
  and clean up failed temps.
- Changed MM account reattachment to mint only on HTTP 404; network, 5xx,
  decoding, authentication, and corrupt local-state failures now fail closed.
- Made failure to checkpoint a newly returned Polymarket MM account fatal
  instead of continuing with an identity guaranteed to be lost on restart.
- Added stable non-leaking 503 mappings for actor and persistence availability.
- Made API startup return contextual errors for telemetry, durable-directory,
  oracle-bootstrap, read-model, listener, and server failures.
- Made a configured event snapshot directory a startup requirement.
- Supervised the history server and fallible Ctrl-C/SIGTERM future together,
  treating unexpected clean server exit as failure and stopping the projector
  before returning.

No consensus encoding, state-transition rule, integer arithmetic, signing
domain, guest code, proof public input, or deployment pin changed in this
cluster. No deployment occurred.

## Executable evidence

The local regression evidence includes:

- stable, normalized, bounded, domain-separated Polymarket creation-key tests;
- mapping JSON round-trip, schema/genesis binding, and atomic-publication tests;
- API error conversion assertions for exact 503 status/code and suppression of
  the underlying persistence message;
- API raw-event PUT/GET integration coverage through the durable writer;
- OpenAPI drift coverage after the new stable errors;
- history package/bin compilation and projector/store tests;
- existing sequencer crash-point/WAL recovery tests;
- existing history duplicate/gap/conflict transactional tests;
- existing prover crash-after-publish/before-DB adoption test; and
- existing L1 cursor and integrity-latch tests.

Filesystem `sync_all` cannot by itself simulate a kernel/power-loss matrix.
The reusable deterministic injection framework and remaining file migrations
are deliberately acceptance criteria in #189 rather than claimed as covered.

## Verification

Passed after the final fixes:

- `cargo test -p sybil-polymarket -p sybil-history -p sybil-native --all-targets`
  — 68 tests;
- `cargo test -p sybil-client -p sybil-polymarket -p sybil-native --all-targets`
  — 71 tests, including the authoritative-API-status regression;
- `cargo test -p sybil-api --lib` — 58 tests;
- `cargo test -p sybil-api --test api_integration event_raw_snapshot_put_then_get`
  — the durable snapshot PUT/GET path;
- `cargo test -p sybil-api --test openapi_drift` — 9 tests;
- strict all-target/all-feature Clippy with `-D warnings` for `sybil-api`,
  `sybil-client`, `sybil-history`, `sybil-native`, and
  `sybil-polymarket`;
- `cargo fmt --all -- --check`; and
- `just docs-check`, including protocol-pin synchronization, repository/doc
  inventory, vault/link/runbook validation, and strict MkDocs build.

The first changed-package compile caught a missing comma in the refactored
history `select!`; it was corrected before the complete passing run. No failed
gate is being hidden as a pass.

## Tracked work

- [#188 — Make service account provisioning idempotent across ambiguous
  responses](https://github.com/MetaB0y/sybil/issues/188): Project 1
  `Todo` / `Backlog` / `High`.
- [#189 — Consolidate crash-safe local file persistence and recovery
  tests](https://github.com/MetaB0y/sybil/issues/189): Project 1
  `Todo` / `Backlog` / `Medium`.
- [#129 — Give native market groups protocol-level creation
  identity](https://github.com/MetaB0y/sybil/issues/129): pre-existing Project 1
  item; updated with the Polymarket crash/recovery witness.
- [#184 — Put a hard backpressure boundary in front of the sequencer actor
  mailbox](https://github.com/MetaB0y/sybil/issues/184): pre-existing broader
  accepted-write receipt and ambiguity work; #188 specifies the concrete
  service-account provisioning contract.

## Residual risk and completion

The bounded defects in the cluster are resolved, but retry safety is not
complete until #188 and #129 provide protocol identities for account and group
creation. Files covered by #189 must not be represented as power-loss durable
until their common primitive and fault matrix land. Production invariant
assertions remain intentionally fail-stop; the next static-lint/unsafe-policy
cluster should encode a reviewed allowlist so future environment-dependent
`unwrap`/`expect` sites cannot blend into that invariant set.

The bounded cluster is complete: its changed-package tests, strict Clippy,
formatting, focused API integration/OpenAPI tests, documentation checks, report,
issues, Project 1 metadata, and timestamped collaboration log are published.
