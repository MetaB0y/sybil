---
tags: [audit, code-quality, session-report, rust, api, testing, operations]
layer: cross-cutting
status: current
date: 2026-07-17
last_verified: 2026-07-17
---

# Code-quality audit session report — 2026-07-17

## Time and accounting

The persisted goal meter at the capacity stop recorded:

- agent work time: **13,892 seconds = 3 hours, 51 minutes, 32 seconds**;
- model-token usage: **3,679,398 tokens**;
- state at interruption: the sixth cluster, error/panic/durable recovery, was
  implemented but its final edge-case review, report, issue publication, and
  verification gates were incomplete.

The later continuation finished that sixth cluster. Its additional wall/agent
time is not included in the frozen 13,892-second interruption measurement, so
this report does not present a false combined precision.

No review subagent was used in these clusters. The work required one reviewer
to preserve cross-language integer semantics, validity boundaries, economic
oracle independence, stream replay semantics, acknowledged-write ownership,
and retry/durability classification consistently. No deployment was performed.

## Outcome

The session completed six repository-aware audit clusters:

1. cross-language exact nanodollar wire fidelity;
2. validity-core mutation and test-oracle effectiveness;
3. adversarial economic/mechanism properties;
4. stateful API and generated-client conformance;
5. actor lifecycle, cancellation, supervision, and backpressure; and
6. error, panic, retry, and durable recovery boundaries.

Each cluster has a dated comprehensive report, executable evidence, code or test
remediation, architecture documentation updates, and deduplicated GitHub work.
The session opened issues
[#177](https://github.com/MetaB0y/sybil/issues/177) through
[#189](https://github.com/MetaB0y/sybil/issues/189) and updated the pre-existing
market-group identity issue
[#129](https://github.com/MetaB0y/sybil/issues/129). All new issues were added
to private Project 1 with explicit workflow stage and priority.

The living program index now advances to static lint, dead code, and unsafe-code
policy. The work deliberately did not claim that the entire original
open-ended audit program was exhausted; it completed the last in-progress
cluster and left the next clusters chartered.

## Work sequence

The collaboration log records the durable completion points:

| Completion time (Europe/London) | Cluster |
|---|---|
| 12:52:32 | Exact wire fidelity |
| 14:27:25 | Validity-core mutation |
| 15:02:16 | Economic properties |
| 15:30:52 | API/client conformance |
| 16:06:12 | Actor lifecycle/supervision |
| 20:22:29 | Error/retry/recovery continuation |

These are handoff timestamps, not per-cluster duration measurements. Compilation,
mutation campaigns, OpenVM rebuilds, generated-client validation, research, and
documentation gates overlapped the reasoning work.

## 1. Exact nanodollar wire fidelity

Report:
[`code-quality-audit-exact-wire-2026-07-17.md`](code-quality-audit-exact-wire-2026-07-17.md)

### What was audited

- Every REST and WebSocket `*_nanos` field across Rust DTOs, OpenAPI, generated
  Python, TypeScript/browser clients, Arena, and handwritten conversion code.
- Nested maps/arrays, signed request inputs, response outputs, schema
  generation, and replay payloads.
- The JavaScript `2^53 - 1` precision boundary versus the protocol's 64-bit
  integer domain.

### What changed

- Introduced one Rust wire-integer serializer/deserializer contract: emit exact
  base-10 JSON strings while accepting legacy integer tokens during migration.
- Applied it to monetary fields, including nested clearing-price maps.
- Updated OpenAPI to advertise decimal strings rather than unsafe JSON numbers.
- Regenerated Python/TypeScript surfaces without a post-generation bigint patch
  script.
- Updated Python, browser, and Arena parsing/formatting to preserve exact
  integers.
- Added cross-runtime boundary tests around values above JavaScript's safe
  integer range and signed/unsigned extremes.

### Deferred work

- [#177](https://github.com/MetaB0y/sybil/issues/177): extend the exact JSON
  integer policy beyond `*_nanos` to all protocol identifiers/counters that can
  exceed JavaScript's safe range.

## 2. Validity-core mutation and oracle effectiveness

Report:
[`code-quality-audit-validity-mutation-2026-07-17.md`](code-quality-audit-validity-mutation-2026-07-17.md)

### What was audited

- Verifier acceptance/rejection behavior for settlement, account keys, signed
  actions, event commitments, quarantine, and block transition checks.
- Whether existing tests actually killed plausible validity bugs rather than
  merely executing the code.
- Native verifier and OpenVM guest closure/pin consistency after changes.

### What changed

- Added adversarial verifier tests for previously weak mutation classes,
  including account-key transitions, signed-action binding, event commitments,
  quarantine state, fill/settlement relationships, and malformed block facts.
- Strengthened independent expected values and negative assertions instead of
  replaying production helpers as the test oracle.
- Rebuilt the applicable OpenVM guests and updated required validity pins. The
  repository remained in its explicit fresh-genesis/pending-redeploy posture;
  no live deployment was touched.

### Evidence

- 235 targeted mutants were classified:
  - 220 caught;
  - 7 behaviorally equivalent;
  - 8 unviable;
  - 0 unexplained survivors.
- Consensus and guest-rebuild checks passed after the pin update.

### Deferred work

- [#178](https://github.com/MetaB0y/sybil/issues/178): separate verifier
  validity policy from diagnostic-quality policy.
- [#179](https://github.com/MetaB0y/sybil/issues/179): add explicit
  action-domain prefixes to canonical signing bytes.

## 3. Adversarial economic and mechanism properties

Report:
[`code-quality-audit-economic-properties-2026-07-17.md`](code-quality-audit-economic-properties-2026-07-17.md)

### What was audited

- Fill feasibility, uniform clearing prices, limit-price constraints,
  conservation, participant surplus, MM capital, mint/burn lifecycle, and
  retained-cash certificate behavior.
- The difference between production one-hot admitted orders and research-only
  general payoff-vector generators.
- Independence of test oracles from settlement/minting helpers under test.

### What changed

- Replaced a shared production settlement replay with an independent,
  checked-integer one-hot oracle.
- Added complete fill-feasibility, UCP, surplus, conservation, and MM-capital
  properties.
- Added a generated complete-set mint-then-burn lifecycle.
- Removed an order-dependent coverage diagnostic that could always pass while
  displaying misleading zero coverage.
- Fixed a retained-cash solver false `NumericalFailure` at exact structural
  breakpoints with an evaluation-roundoff bound that still rejects a material
  certificate gap.
- Tightened feature gates for conic-only helpers found by strict Clippy.

### Evidence

- Production HiGHS and experimental structural profiles each passed 2,048
  generated zero-tolerance cases.
- Retained-cash, LP, conic, sequencer, formatting, and strict lint profiles
  passed.

### Deferred work

- [#180](https://github.com/MetaB0y/sybil/issues/180): create the independent
  temporal sell-reservation state-machine model.

## 4. Stateful API and generated-client conformance

Report:
[`code-quality-audit-api-client-conformance-2026-07-17.md`](code-quality-audit-api-client-conformance-2026-07-17.md)

### What was audited

- Canonical OpenAPI versus runtime route profiles.
- Rust, Python, and TypeScript generated/handwritten clients.
- Error status/documentation parity.
- WebSocket version/type evolution, replay classification, retention gaps, and
  side-effecting consumers.
- Schema-derived request testing and dependency-aware state sequences.

### What changed

- Validated 69 paths, 75 unique operations, 116 schemas, and zero OpenAPI links.
- Documented the missing `410 RETENTION_GONE` response and corrected an invalid
  P-256 example.
- Replaced profile-dependent Python SDK scraping with deterministic canonical
  schema generation; the generated substrate grew from 70 to all 75 operations.
- Made the Rust stream decoder ignore unknown versions/types before decoding a
  known payload while retaining hard failure for malformed known messages.
- Added replay-aware Python events and prevented Arena bots/analysts from
  performing historical side effects.
- Made browser retention recovery fail closed until a fresh REST snapshot is
  installed and corrected the initial replay boundary.
- Ran a disposable schema-fuzzer campaign and classified its failures by
  contract/precondition rather than reporting a raw failure count.

### Deferred work

- [#181](https://github.com/MetaB0y/sybil/issues/181): make runtime request
  rejection constraints executable in the schema/conformance pipeline.
- [#182](https://github.com/MetaB0y/sybil/issues/182): add dependency-aware
  stateful REST sequences.
- [#183](https://github.com/MetaB0y/sybil/issues/183): define a
  machine-readable cross-runtime WebSocket contract.

## 5. Actor lifecycle, cancellation, and supervision

Report:
[`code-quality-audit-actor-lifecycle-2026-07-17.md`](code-quality-audit-actor-lifecycle-2026-07-17.md)

### What was audited

- Task/actor owner, channel capacity, cancellation source, failure observer,
  join/escalation path, and process-exit behavior across the sequencer, API,
  history, prover, L1 indexer, native and Polymarket integrations, market
  maker, and native verifier workers.
- Every await in the relevant long-running loops.
- Safe-to-cancel reads/speculation versus already-started non-idempotent writes.
- Docker SIGTERM, readiness versus supervision, retry sleeps, WebSocket
  handshakes, and blocking-task ownership.

### What changed

- Added Ctrl-C/SIGTERM handling to Polymarket, native MM, and L1 indexer.
- Made shared-MM WebSocket connect, retry sleep, and read-only refresh
  cancellation-aware while allowing already-started order writes to resolve.
- Added cancellation safe points to Polymarket sync/feed/resolution loops.
- Converted unexpected clean/error/panic exits of critical integration tasks
  into nonzero process failures.
- Made prover scheduler, source, and HTTP server one process-supervision unit so
  Compose can restart a failed daemon.
- Added L1 request deadlines and graceful monitoring shutdown, including its
  fatal metrics-only posture.
- Replaced unbounded native verifier state/event root queues with bounded
  synchronous queues of 64.
- Added a minimized stalled-WebSocket-handshake cancellation regression test.

### Deferred work

- [#184](https://github.com/MetaB0y/sybil/issues/184): hard pre-mailbox
  sequencer backpressure and unambiguous accepted-write receipts.
- [#185](https://github.com/MetaB0y/sybil/issues/185): process escalation after
  terminal canonical-owner restart failure.
- [#186](https://github.com/MetaB0y/sybil/issues/186): deterministic
  signal/cancellation/supervisor/saturation matrix.
- [#187](https://github.com/MetaB0y/sybil/issues/187): bounded ownership and
  escalation for blocking/post-commit work.

## 6. Error, panic, retry, and durable recovery

Report:
[`code-quality-audit-error-recovery-2026-07-17.md`](code-quality-audit-error-recovery-2026-07-17.md)

### What was audited

- A diagnostic inventory of 139 `unwrap`/`expect`/`panic` warning instances
  across selected production targets and local workspace dependencies.
- Error identity from source through API response, log, supervisor, and process
  result.
- Durable operation boundaries for sequencer, history, prover, L1 cursor,
  Polymarket mapping, event snapshots, native state, and off-block metadata.
- Safe read retries, stable create identities, response-loss ambiguity,
  integrity latches, and fail-open/fail-closed startup behavior.

### What changed

- Corrected MM reattachment: only an authoritative API 404 permits account
  replacement. Network, decode, authentication, and 5xx failures now fail
  closed; corrupt native state no longer silently mints another account.
- Added deterministic, normalized, domain-separated Polymarket market creation
  keys.
- Checkpointed each remote market/group/extension/event result immediately.
- Added recovery/adoption for a compatible group left by a crash before its
  local checkpoint.
- Made Polymarket mapping and API raw-event snapshot publication sync contents,
  atomically rename, and sync the parent directory; snapshots use unique
  timestamp/PID/nonce temp names and blocking I/O off the async executor.
- Added stable non-leaking `503 SEQUENCER_UNAVAILABLE` and
  `503 SEQUENCER_PERSISTENCE_UNAVAILABLE` responses.
- Replaced recoverable API startup panics/unwraps with contextual returned
  errors and made configured snapshot persistence fail closed.
- Made history signal registration fallible, supervised unexpected HTTP exit,
  and ensured projector shutdown precedes returned server/signal errors.

### Interruption and resumed completion

At the capacity stop, most code above existed, but two review findings remained:

- the history `select!` branches used `?`, which would return from `main` before
  stopping the projector on an error; and
- event snapshot temp identity needed to tolerate stale files after PID reuse.

The continuation fixed both, added the authoritative-HTTP-status client helper
and regression test, reran the package/integration gates, published the report,
updated architecture and program documentation, and created/projected the open
issues below.

### Deferred work

- [#188](https://github.com/MetaB0y/sybil/issues/188): idempotent funded
  service-account provisioning across a commit/response/checkpoint crash
  window. Project 1 Todo / Backlog / High.
- [#189](https://github.com/MetaB0y/sybil/issues/189): one crash-safe local-file
  primitive and deterministic fault matrix. Todo / Backlog / Medium.
- [#129](https://github.com/MetaB0y/sybil/issues/129): protocol-level
  market-group creation identity. The issue was updated with the new
  Polymarket crash witness.

## Research and review technique

The work did not apply generic “AI code review” prompts uncritically. It used a
repository-specific evidence hierarchy:

1. independent cross-runtime/cross-implementation equality;
2. generated properties from protocol invariants;
3. deterministic known-answer vectors;
4. static feasible paths with a minimized witness;
5. mutation results proving an oracle notices a changed behavior;
6. contextualized linter findings; and
7. model-only suggestions only as leads.

Online research covered repository-aware AI review and validation, REST
schema/stateful testing, Rust mutation testing, Tokio shutdown/cancellation/task
ownership, actor patterns, Rust panic/error guidance, Clippy restriction-lint
policy, redb recovery/transactions, and idempotent API retries. Each cluster
report links the primary source that materially affected its method.

The recurring workflow was:

- read applicable `AGENTS.md` and architecture notes;
- define a narrow cluster and evidence boundary;
- inventory the semantic paths before accepting tool output;
- reproduce or independently oracle-check each concrete finding;
- fix bounded bugs and add regression evidence;
- deduplicate architectural work against GitHub Issues;
- add Project 1 stage/priority metadata;
- update current architecture documentation;
- run proportionate tests, lints, generators, consensus/docs gates; and
- record the result in the timestamped collaboration log.

## Verification summary

The detailed reports contain exact command-level evidence. Across the six
clusters, the successful gates included:

- targeted and full changed-package Rust tests for the matching engine,
  solvers, sequencer, verifier, API, client, history, prover, L1 indexer,
  native/Polymarket integrations, and market maker;
- 235-mutant validity classification with no unexplained survivor;
- 4,096 generated economic cases across two solver profiles;
- OpenAPI route/operation/schema inventories and drift tests;
- generated Python SDK, Arena pytest/Ruff/formatting checks;
- frontend TypeScript, ESLint, Vitest, and generation checks;
- cancellation/supervisor regression tests;
- strict changed-package all-target/all-feature Clippy;
- Rust formatting;
- verifier/guest/consensus pin checks where validity code changed;
- Compose/profile checks where process topology changed; and
- strict documentation/vault/site validation.

The final recovery-cluster gates are recorded in its own report after the last
run. No production deploy, live provider mutation, L1 submission, or real proof
generation was used as evidence.

## Artifact index

- Living program:
  [`code-quality-audit-program-2026-07.md`](code-quality-audit-program-2026-07.md)
- Timestamped method/decision log:
  [`code-quality-audit-collaboration-log-2026-07.md`](code-quality-audit-collaboration-log-2026-07.md)
- Exact wire report:
  [`code-quality-audit-exact-wire-2026-07-17.md`](code-quality-audit-exact-wire-2026-07-17.md)
- Validity mutation report:
  [`code-quality-audit-validity-mutation-2026-07-17.md`](code-quality-audit-validity-mutation-2026-07-17.md)
- Economic properties report:
  [`code-quality-audit-economic-properties-2026-07-17.md`](code-quality-audit-economic-properties-2026-07-17.md)
- API/client conformance report:
  [`code-quality-audit-api-client-conformance-2026-07-17.md`](code-quality-audit-api-client-conformance-2026-07-17.md)
- Actor lifecycle report:
  [`code-quality-audit-actor-lifecycle-2026-07-17.md`](code-quality-audit-actor-lifecycle-2026-07-17.md)
- Error/recovery report:
  [`code-quality-audit-error-recovery-2026-07-17.md`](code-quality-audit-error-recovery-2026-07-17.md)

## Repository state and handoff

All work remains in the shared Jujutsu working change
`lqnmlkmm` (`fix(api): serialize nanos as exact JSON strings`). The working copy
also contains the earlier audit clusters and pre-existing related edits; no
unrelated change was reset or discarded. Because the repository uses Jujutsu,
status/diff inspection used `jj`, not Git.

The next planned cluster is static lint, dead code, and unsafe-code policy. Its
charter is in the living program report. It was not started as part of this
“finish the last item” continuation.
