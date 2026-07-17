---
tags: [audit, code-quality, rust, actors, async, operations]
layer: cross-cutting
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Actor lifecycle, cancellation, supervision, and backpressure audit

Date: 2026-07-17  
Cluster: production Rust actors, background tasks, channels, cancellation,
process signals, restart semantics, and blocking-work ownership  
Primary technique: task-ownership matrix plus cancellation-point and
supervisor-failure analysis

## Verdict

The repository already had several strong lifecycle primitives: one canonical
sequencer actor, durable restart from its store, coalesced scheduler ticks,
bounded explicit Tokio channels, `CancellationToken`/`TaskTracker` ownership in
the API and Polymarket integration, graceful Axum shutdown in core services,
and idempotent durable outboxes.

The surrounding process supervision was inconsistent. The concrete audit found
and fixed six defects:

- the Polymarket and native-MM binaries listened only for Ctrl-C, so Docker
  SIGTERM skipped their cancellation and join paths;
- the shared MM could wait indefinitely in a WebSocket handshake or an
  unconditional five-second retry sleep after cancellation;
- clean or failed early exit from required Polymarket/native-MM tasks still
  returned process success;
- Polymarket catalog/feed/resolution cycles did not consistently observe
  cancellation between safe I/O and side-effect boundaries;
- the prover marked itself unready after scheduler/source failure but kept its
  HTTP process alive, so Compose's `on-failure` policy could never restart it;
  and
- the L1 indexer had neither process-signal shutdown nor outbound request
  deadlines, allowing one silent provider to freeze polling behind a stale
  readiness snapshot.

The native qMDB state/event root workers also used unbounded standard-library
queues. They are now bounded at 64 pending jobs; root computation remains
serialized and deterministic.

Three important ownership gaps remain. The sequencer's external ractor mailbox
has monitoring but no hard admission bound, an unrecoverable child-restart
failure leaves the API process alive without an actor, and blocking/post-commit
tasks do not yet have a complete escalation contract. These are tracked in
[GitHub #184](https://github.com/MetaB0y/sybil/issues/184),
[#185](https://github.com/MetaB0y/sybil/issues/185), and
[#187](https://github.com/MetaB0y/sybil/issues/187). A deterministic
process-signal and lifecycle fault matrix is
[#186](https://github.com/MetaB0y/sybil/issues/186).

## Evidence boundary

The audit covered production ownership and communication paths in:

- `matching-sequencer` actor, supervisor, scheduled/indicative work, qMDB
  service threads, broadcast stream, and shutdown handle;
- `sybil-api` Axum server, history/read-model/process workers, TaskTracker
  ordering, and sequencer shutdown;
- `sybil-history` projector actor, blocking query pool, apply timeout, and
  process shutdown;
- `sybil-prover` scheduler, source puller, proof subprocess cancellation,
  HTTP server, and watch-based shutdown;
- `sybil-market-maker`, `sybil-polymarket`, and `sybil-native` channels,
  WebSocket/HTTP loops, side effects, monitoring, and process supervision;
- `sybil-l1-indexer` polling, fatal metrics-only mode, HTTP transports,
  monitoring server, and cursor boundary; and
- native-only `sybil-verifier` qMDB root worker threads.

The review classified explicit channel capacity, but did not claim that ractor's
internal mailbox is bounded. It inspected cancellation paths and ran local
package tests; it did not deploy, send a signal to a production process, stall
a real L1 provider, or run a real OpenVM proof.

## Architecture context read

The review used the root guidance and the crate guidance for the sequencer,
API, history, prover, Polymarket, native MM, market maker, L1 indexer, and
verifier. Relevant current architecture included:

- `Actor Mailbox Monitoring`
- `Block Lifecycle`
- `Persistence`
- `Acknowledged-Write WAL Replay`
- `Historical Data Serving`
- `WebSocket Block Stream`
- `Deployment Profiles`
- `Data Availability`
- `ZK Integration Path`
- `Block Witness`
- `Four-Layer Verification`
- `State Root Schema`
- `L1 Settlement and Vault`

The controlling invariants were:

- the sequencer actor is the only canonical exchange-state writer;
- acknowledged writes must remain durable and replayable across ambiguous
  transport outcomes;
- history acknowledgement follows durable projection;
- proof-job acknowledgement transfers durable ownership only after prover
  commit;
- read-only/speculative work may be cancelled, while already-started
  acknowledged writes need an unambiguous completion or recovery contract; and
- fatal integrity state must remain observable without silently resuming work.

## Research basis and method

- Tokio's [graceful-shutdown guide](https://tokio.rs/tokio/topics/shutdown)
  separates detection, notification, and waiting. The inventory therefore
  required an owner for all three phases, rather than accepting the presence of
  a cancellation token as sufficient.
- The official
  [`CancellationToken` documentation](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)
  says `run_until_cancelled` is safe only when the wrapped future is
  cancellation-safe. The fixes cancel read-only connection/fetch work, but
  deliberately await an already-started order or resolution submission.
- Tokio's
  [`select!` cancellation-safety guidance](https://docs.rs/tokio/latest/tokio/macro.select.html)
  motivated inspection of every loop await and use of `biased;` where shutdown
  must win over simultaneously ready work.
- The
  [`TaskTracker` contract](https://docs.rs/tokio-util/latest/tokio_util/task/struct.TaskTracker.html)
  requires both close and task completion. A timed-out wait does not terminate
  tracked work; this directly produced #187.
- Tokio's [Loom project](https://github.com/tokio-rs/loom) supports exhaustive
  small synchronization-state exploration. It is proposed only for the
  repository's small atomic gates/accounting kernels, not for storage or
  network integration.
- Alice Ryhl's
  [Tokio actor pattern](https://ryhl.io/blog/actors-with-tokio/) reinforces the
  existing repository rule that an actor handle owns the mailbox and the
  spawned task owns mutable state. The review extended that ownership question
  to task exit and process supervision.

The audit procedure was:

1. Inventory actors, `spawn`, `spawn_blocking`, dedicated threads, channel
   capacities, join handles, tokens, timers, retry loops, and signal hooks.
2. Map each task to its start owner, communication path, failure observer,
   cancellation source, and join/escalation path.
3. Trace cancellation through every await before classifying a loop as
   shutdown-aware.
4. Separate safe-to-drop reads/speculation from writes whose transport result
   can be ambiguous.
5. Check whether child exit changes process exit status; readiness alone is not
   process supervision.
6. Check explicit capacity before accepting monitoring or rate limiting as
   backpressure.
7. Add minimized lifecycle tests and bounded fixes where the ownership contract
   was local.
8. File architectural work where a timeout or small queue patch would violate
   acknowledged-write semantics.

## Ownership matrix

| Component | Work owner and communication | Capacity / overload | Shutdown and failure contract | Audit result |
|---|---|---|---|---|
| Sequencer | ractor supervisor + linked canonical child; RPC reply ports | Scheduled and indicative work coalesced; block broadcast 64; external actor mailbox not hard-bounded | Durable child restart; explicit stop deadline; terminal restart failure only logs | Strong core, #184/#185/#187 open |
| qMDB account/state services | Dedicated Commonware threads; Tokio mpsc + oneshot | Both command queues bounded at 8 | Sender closure ends service; individual calls await response | Accepted |
| API workers | CancellationToken + TaskTracker for metrics, history, leaderboard | HTTP-specific semaphores; actor RPC admission remains downstream | SIGTERM/Ctrl-C drains Axum, cancels/waits workers, then stops sequencer | Correct order; blocking remainder #187 |
| History | ractor single-writer projector; spawn_blocking redb; query semaphore | Apply RPC timeout 30s; query concurrency 16 | SIGTERM/Ctrl-C drains HTTP and stops projector | Accepted; blocking remainder #187 |
| Prover daemon | scheduler + source JoinHandles, watch shutdown, kill-on-drop backend, Axum | One proof lease; bounded retries/timeouts in backend/source policy | Any child/server early exit now stops siblings and exits nonzero; signals drain all | Fixed |
| Polymarket integration | TaskTracker owns sync/feed/MM/resolution/monitoring; bounded mpsc and watch | feed 64; MM sized to configured catalog; HTTP 30s | SIGTERM/Ctrl-C, cancellation-safe read/retry points, 35s join, 40s Compose grace; critical exit nonzero | Fixed |
| Native MM | actor + monitoring JoinHandles; bounded mpsc/watch | channel sized to provisioned market set | Same signal/deadline/error policy as shared MM; already-started order submit finishes | Fixed |
| Shared MM | one mutable actor over public block stream | bounded input channel owned by caller; rotating quote cap | cancellation wins connect/retry/read-only refresh; no new quote submit after observed cancel | Fixed and regression-tested |
| L1 indexer | pinned poll/server futures; token-owned monitoring | provider set fixed by source identity; request timeout 30s | SIGTERM/Ctrl-C cancels poll and drains server; fatal metrics mode remains signal-aware | Fixed |
| Verifier roots | two process-lifetime native qMDB worker threads | state/event request queues now bounded at 64 | synchronous callers await exact response; thread lifetime is process lifetime | Fixed |
| Public block fanout | Tokio broadcast 64 + retained replay store | explicit lag and retention-gap protocol | slow consumers resync or fail closed; client behavior audited separately | Accepted from prior API cluster |

All explicit Tokio mpsc command channels in the scoped production paths are
bounded. The remaining unbounded data-plane is the ractor mailbox described in
#184; its depth metric is observability, not admission control.

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| AL-1 | High | Polymarket and native-MM ignored Docker SIGTERM, bypassing token cancellation and task joins. | Fixed with Ctrl-C/SIGTERM futures and documented grace budgets. |
| AL-2 | High | The shared MM could remain in a pending WebSocket handshake or unconditional retry sleep after shutdown, and a simultaneously ready block could beat cancellation. | Fixed with cancellation-selected connect/retry/read work, biased shutdown branches, and a stalled-handshake regression test. |
| AL-3 | High | Required Polymarket/native actor or monitoring tasks could return and the process still exited success. | Fixed: clean early return, error, or panic is a critical nonzero process exit after sibling shutdown. |
| AL-4 | Medium | Long Polymarket sync/feed/resolution cycles checked cancellation only between whole cycles. | Fixed with safe-point checks and cancellable read-only fetches; already-started non-idempotent writes finish. |
| AL-5 | High | Prover scheduler/source failure only cleared readiness while leaving HTTP alive, defeating `restart: on-failure`. | Fixed with one process supervisor over scheduler, source, server, and signal. |
| AL-6 | High | L1 indexer had no signal path and no request deadline; one silent provider could freeze a live, previously-ready process. | Fixed with SIGTERM/Ctrl-C, graceful monitoring cancellation, and configurable 30s request timeout. |
| AL-7 | Medium | Native verifier state/event root workers used unbounded request queues for block-sized allocations. | Fixed with bounded synchronous queues of 64. |
| AL-8 | High | Sequencer external RPCs enter an unbounded mailbox and have no deadline; rate limiting occurs after enqueue. | Open as #184, Project 1 Todo/Backlog/High. |
| AL-9 | High | Terminal durable-restart failures leave the API process alive with no canonical actor. | Open as #185, Project 1 Todo/Backlog/High. |
| AL-10 | Medium | Lifecycle tests do not systematically inject process signals, child exits, simultaneous cancellation, full queues, or virtual-time retries. | Open as #186, Project 1 Todo/Backlog/Medium. |
| AL-11 | Medium | Timed-out TaskTracker waits and unabortable `spawn_blocking` work lack one explicit process escalation contract. | Open as #187, Project 1 Todo/Backlog/Medium. |

## Why some work is cancelled and some is awaited

Cancellation is not equivalent to rollback. The implemented boundary is:

- WebSocket connect, retry sleep, REST reads, price refresh, and speculative
  state sync may be dropped when cancellation wins.
- A new quote, market, group extension, resolution, or other side effect is not
  started after cancellation has been observed.
- Once a non-idempotent submission has started, the actor awaits its bounded
  response before exiting. Dropping that future could allow the server to
  accept the write while the caller reports shutdown, producing an ambiguous
  retry.
- Durable mapping/cursor writes remain atomic and completed side effects are
  either recorded locally or replayed through an idempotent API boundary.

This is why the fixes did not wrap every future in one broad
`run_until_cancelled`: doing so would improve apparent shutdown latency by
weakening correctness.

## Implemented changes

- Added SIGTERM plus Ctrl-C handling to Polymarket, native MM, and L1 indexer.
- Made shared-MM block-stream connection and retry waits cancellation-aware.
- Prioritized cancellation over ready data branches with `biased;` where a
  post-cancel side effect would be possible.
- Added cancellation-safe read-only refreshes and safe-point checks to shared
  MM, feed, sync, and resolution loops.
- Preserved completion of already-started MM order and resolution writes.
- Added 35-second process task deadlines inside 40-second Compose grace periods
  for the integration services.
- Converted clean/failed early task returns into nonzero process failures.
- Supervised prover scheduler, source, and server as one process unit.
- Added graceful monitoring shutdown and a 30-second configurable transport
  timeout to the L1 indexer.
- Bounded native verifier root-worker queues at 64.
- Updated current deployment, L1, ZK, and prover-runbook documentation.

No consensus bytes, state-transition rules, guest code, signing domains, or
deployment pins changed in this cluster.

## Executable evidence

The key minimized reproducer is
`cancellation_interrupts_pending_block_stream_handshake`: a local TCP listener
accepts the MM's socket and deliberately never completes the WebSocket
handshake. Cancelling the actor must still join within 250 ms while that socket
remains open. The pre-fix actor would wait for the handshake.

Additional local assertions cover:

- L1 monitoring server completion after token cancellation;
- prover classification of a clean critical-child return as process failure;
- existing sequencer restart, in-flight tick drain, and scheduled-tick
  coalescing behavior;
- Polymarket/native monitoring readiness and actor progress; and
- verifier root determinism under the bounded worker transport.

The broader real-process signal matrix remains #186 rather than being simulated
with sleeps inside unrelated unit tests.

## Verification

Passed:

- `cargo test -p sybil-market-maker -p sybil-polymarket -p sybil-native -p sybil-l1-indexer --all-targets`
  — 128 tests;
- `cargo test -p sybil-verifier` — 149 tests, 1 ignored doctest;
- `cargo test -p sybil-prover --all-features`;
- focused sequencer, API, and history lifecycle/package tests. The first
  all-target invocation ran five API child-process restart cases concurrently
  and three health probes timed out under local resource contention; all five
  passed in 4.39 seconds when rerun serially with `--test-threads=1`;
- strict all-target/all-feature Clippy for every changed Rust package;
- Rust formatting;
- verifier golden checks;
- effective Compose/profile checks; and
- `just docs-check`, including strict documentation/site validation.

No deployment was performed.

## Open issues and project state

| Issue | Purpose | Project 1 |
|---|---|---|
| [#184](https://github.com/MetaB0y/sybil/issues/184) | Hard pre-mailbox backpressure plus unambiguous write receipts | Todo / Backlog / High |
| [#185](https://github.com/MetaB0y/sybil/issues/185) | Process-level escalation for terminal canonical-owner failures | Todo / Backlog / High |
| [#186](https://github.com/MetaB0y/sybil/issues/186) | Deterministic process-signal, cancellation, supervisor, and saturation tests | Todo / Backlog / Medium |
| [#187](https://github.com/MetaB0y/sybil/issues/187) | Bounded ownership of blocking and post-commit work | Todo / Backlog / Medium |

## Residual risk and completion

The bounded cluster is complete for the concrete integration/prover/indexer
shutdown bugs and verifier queue growth. It does not claim that the canonical
sequencer can absorb arbitrary HTTP concurrency: #184 is the required
pre-mailbox design, and it must solve write ambiguity rather than merely adding
a timeout. Nor does it claim complete process supervision while #185 and #187
remain open.

The next code-quality cluster should be error, panic, and recovery boundaries.
It should reuse this ownership matrix to distinguish an intentional integrity
fail-stop from an accidental `unwrap`, retry loop, partial write, or
error-identity collapse.
