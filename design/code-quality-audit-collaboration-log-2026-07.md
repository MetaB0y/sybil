---
tags: [audit, code-quality, collaboration, codex]
layer: cross-cutting
status: current
date: 2026-07-17
last_verified: 2026-07-17
---

# Code-quality audit collaboration log — July 2026

This log records durable context shared between the primary reviewer and
review agents. It is a decision/handoff log, not a transcript and not an issue
tracker.

## 2026-07-17T12:52:32+01:00 — exact-wire cluster

### Primary reviewer → review agents

- The repository uses Jujutsu, not Git; GitHub Issues in `MetaB0y/sybil` are
  authoritative.
- Treat the large existing `fix(api): serialize nanos as exact JSON strings`
  working change as shared in-flight work. Do not overwrite unrelated edits.
- Inventory existing audit artifacts and uncovered clusters independently.
- Research evidence-backed AI/static/dynamic review methods and challenge
  whether mutation testing should be the first technique.
- Do not edit files and do not spawn additional agents for these inventory
  tasks.

### `audit_inventory` → primary reviewer

- No dedicated code-quality program index or collaboration log existed.
- Existing durable audits concentrate on DoS/resource growth and
  repository/simplicity boundaries.
- The strongest untouched cluster is test-oracle effectiveness in the validity
  core, using targeted mutation testing; adversarial economic properties are
  the next broad candidate.
- Existing reports and issues must be reconciled before creating new backlog.
- No files were changed and no subagent was spawned.

### `review_research` → primary reviewer

- Cross-language exact-wire/API fidelity is the strongest immediate cluster
  because the current working change already exposes a concrete Rust/OpenAPI/
  Python/TypeScript seam.
- Standards support strings or executable safe bounds for 64-bit integers:
  RFC 8259, RFC 7493, and the OpenAPI `int64`/`uint64` registries.
- Recommended evidence hierarchy:
  cross-runtime differential round trips; schema-derived property/stateful
  tests; known-answer signing/hash/ABI vectors; OpenAPI drift; then mutation as
  a meta-test.
- Minimum future integer corpus should include `0`, `1`, 32-bit boundaries,
  `2^53 - 1`, `2^53`, `2^53 + 1`, signed/unsigned extrema, domain max/max+1,
  invalid signs, fractions, exponents, whitespace, null/missing, and
  number-versus-string tokens.
- Many non-nanodollar protocol integers remain numeric and need a separate
  policy rather than silent expansion of the current migration.
- No files were changed and no subagent was spawned.

### Primary-reviewer decision shared back into the program

- Finish the in-flight exact-nanodollar contract first because it has a concrete
  cross-runtime failure mode and bounded completion criteria.
- Use semantic data-flow inventory plus executable OpenAPI/runtime invariants;
  do not use mutation as the primary proof for wire fidelity.
- Rename the five nanodollar aliases so the mechanical `*_nanos` rule becomes
  enforceable.
- Keep legacy integer-token input compatibility, but canonicalize output to
  strings and reject unsafe JavaScript compatibility numbers.
- Track the broader 64-bit policy separately as
  [GitHub #177](https://github.com/MetaB0y/sybil/issues/177).
- Run the validity-core mutation cluster next, using a calibrated module subset
  and survivor classification rather than a target mutation score.

### Result

- Exact-wire report:
  [`code-quality-audit-exact-wire-2026-07-17.md`](code-quality-audit-exact-wire-2026-07-17.md)
- Program index:
  [`code-quality-audit-program-2026-07.md`](code-quality-audit-program-2026-07.md)
- No review agent edited the shared worktree.

## 2026-07-17T14:27:25+01:00 — validity-core mutation cluster

### Primary reviewer → `audit_inventory`

- Reuse the existing review agent for one independent, read-only prioritization
  of deterministic validity-core mutation targets.
- Read the focused crate guidance and report high-impact pure functions,
  expected equivalent-mutant traps, existing negative tests, and feature/test
  commands.
- Do not edit files and do not spawn another agent.

### `audit_inventory` → primary reviewer

- Prioritize verifier match checks and engine settlement first, then canonical
  client-action binding, quarantine replay, verifier settlement, and account-key
  commitment helpers.
- Expect broad sign/boolean predicates to create equivalent mutants under the
  admitted one-hot order domain; classify them rather than optimizing a score.
- Run nextest-backed package campaigns and test the verifier without default
  features.
- The no-default verifier test build failed because fixtures called a
  qMDB-only event-root helper.
- No files were changed and no subagent was spawned.

### Primary-reviewer decisions and validation

- Installed and used `cargo-mutants 27.1.0` only as a diagnostic oracle check.
- Kept the campaign bounded to eight deterministic function groups and
  classified every final survivor.
- Accepted seven survivors only after tracing them as behaviorally equivalent;
  added tests or code fixes for every meaningful survivor.
- Fixed the fail-open missing-clearing-price UCP path and the general-payoff
  settlement magnitude defect.
- Added direct negative/state-transition tests for client-action bindings,
  quarantine replay, settlement comparison, and canonical key-operation bytes.
- Fixed the qMDB/no-default fixture boundary and passed the full feature matrix.
- Opened [GitHub #178](https://github.com/MetaB0y/sybil/issues/178) for the
  validity-versus-diagnostics policy and
  [GitHub #179](https://github.com/MetaB0y/sybil/issues/179) for explicit
  signing domains/duplicate canonical shapes. Both are Project 1 `Todo`,
  `Backlog`, Priority `High`.
- Rebuilt both OpenVM guest closures. The main executable commitment changed;
  the escape commitment reproduced. Desired pins are `pending_redeploy`, the
  repository boundary is `fresh_genesis`, and no live deployment occurred.

### Result

- Final classification: 220 caught, 7 equivalent, 8 unviable across 235
  targeted mutants; no unexplained survivors.
- Report:
  [`code-quality-audit-validity-mutation-2026-07-17.md`](code-quality-audit-validity-mutation-2026-07-17.md)
- `just zk-rebuild-check` and `just check-consensus` pass.

## 2026-07-17T15:02:16+01:00 — adversarial economic-property cluster

### Primary-reviewer method and reusable handoff

- Keep production one-hot orders separate from research-only general payoff
  vectors. A passing broad generator is not evidence for an input language
  that production admission rejects.
- Treat tests that call production settlement, minting, side-classification,
  or MM-budget helpers as integration checks, not independent economic oracles.
- Express the narrow admitted-domain oracle separately with checked integer
  arithmetic before widening generators.
- Require falsifiability: perturb a valid output and prove the same checker
  rejects overfills, non-UCP prices, limit violations, incoherent price vectors,
  and conservation defects.
- For floating certificates, preserve the raw reported gap. Bound only
  representation error, using absolute term scales and operation count rather
  than an arbitrary business tolerance.
- Persist minimized generated failures even after the implementation is fixed.

No review agent was used for this cluster. The settlement, sequencer, solver,
and verifier paths formed one tightly coupled evidence chain, and the primary
reviewer retained one owner for the independent-oracle boundary.

### Findings and decisions

- Replaced the shared settlement/minting replay with an independent one-hot
  checked-integer oracle.
- Added complete fill feasibility, UCP, participant-surplus, and conservative
  MM-capital checks.
- Added a generated complete-set mint-then-burn lifecycle and direct
  non-zero-trade coverage assertions.
- Removed an order-dependent atomic diagnostic that printed misleading zero
  coverage while always passing.
- Fixed a structural retained-cash false `NumericalFailure` at exact
  breakpoints with a Higham-style evaluation-error bound; kept the raw gap and
  added both roundoff-acceptance and material-gap-rejection tests.
- Production HiGHS and experimental structural profiles each passed 2,048
  zero-tolerance generated cases.
- Strict Clippy exposed conic-only helpers in narrower builds. Their feature
  boundaries now match their sole consumer, and sequencer/LP/conic lint gates
  pass with `-D warnings`.
- Opened [GitHub #180](https://github.com/MetaB0y/sybil/issues/180) for the
  separate sell-reservation state-machine model. It is Project 1 `Todo`,
  `Backlog`, Priority `Medium`.

### Result

- Report:
  [`code-quality-audit-economic-properties-2026-07-17.md`](code-quality-audit-economic-properties-2026-07-17.md)
- Full matching-sequencer, retained-cash, LP, and conic test profiles pass.
- The next cluster is dependency-aware API sequence generation and
  Rust/Python/TypeScript client conformance.

## 2026-07-17T15:30:52+01:00 — stateful API and client-conformance cluster

### Primary-reviewer method and reusable handoff

- Render one canonical full OpenAPI document and distinguish it from the
  runtime route profile; never infer generator completeness from a production
  server scrape.
- Treat isolated schema generation, dependency-aware state sequences, runtime
  error policy, and WebSocket lifecycle as separate oracles.
- For versioned event streams, inspect the version/type header before decoding
  a known payload. Unknown versions/types are forward-compatible; malformed
  known messages remain errors.
- Preserve replay boundaries all the way to side-effecting consumers. A
  block-only convenience iterator is not safe for trading or external calls.
- A retention gap is a state-replacement transition, not a reconnect hint:
  fail-stop, replace state from REST, seed the new height, then resume.
- Classify schema-fuzzer failures by violated contract and runtime
  precondition; raw failure counts are not findings.

No review agent was used. The route registries, generator scripts, and three
client runtimes formed one cross-runtime contract whose fixes required a single
owner.

### Findings and decisions

- Validated 69 paths, 75 unique operations, 116 schemas, and zero OpenAPI links.
- Fixed an undocumented `410 RETENTION_GONE` response and an invalid P-256
  request example.
- Replaced profile-dependent Python SDK scraping; the generated substrate grew
  from 70 to all 75 operation modules.
- Fixed Rust unknown-version/type handling.
- Added replay-aware Python events and prevented Arena bots/analysts from
  performing historical side effects.
- Made browser retention recovery fail closed until a fresh REST snapshot is
  applied; also fixed initial replay classification.
- A disposable Schemathesis run exposed systematic framework rejection/schema
  gaps. Opened [GitHub #181](https://github.com/MetaB0y/sybil/issues/181)
  (Todo/Backlog/High).
- Opened [GitHub #182](https://github.com/MetaB0y/sybil/issues/182) for
  dependency-aware REST sequences and
  [GitHub #183](https://github.com/MetaB0y/sybil/issues/183) for a
  cross-runtime machine-readable WebSocket contract. Both are
  Todo/Backlog/Medium.

### Result

- Report:
  [`code-quality-audit-api-client-conformance-2026-07-17.md`](code-quality-audit-api-client-conformance-2026-07-17.md)
- Rust client/OpenAPI/WebSocket, Arena, full frontend, TypeScript, ESLint,
  Ruff, formatting, and generator gates pass.
- The next cluster is actor lifecycle, cancellation, and supervision.

## 2026-07-17T16:06:12+01:00 — actor lifecycle and supervision cluster

### Primary-reviewer method and reusable handoff

- Treat graceful shutdown as three separate obligations: detect the external or
  internal stop condition, notify every owner, and wait or explicitly escalate.
  A token without a join path is not ownership.
- Inspect every await inside a loop. A cancellation branch after the complete
  cycle does not make connection, retry, or multi-request work cancellable.
- Cancel reads, sleeps, connection attempts, and speculative work. Do not drop
  an already-started non-idempotent write merely to improve shutdown latency;
  await its bounded result or supply a durable idempotency/receipt contract.
- Use `biased;` only when shutdown must win over simultaneously ready work. Do
  not assume the default randomized `select!` order prevents post-cancel side
  effects.
- Readiness is not supervision. If a required child exits, the process owner
  must restart it, enter an explicit terminal posture, or exit nonzero so the
  external supervisor can act.
- A queue-depth metric and an actor-internal rate limiter are not pre-mailbox
  backpressure. Bound admission before enqueue and preserve write-result
  unambiguity.
- A timed-out `TaskTracker::wait` does not abort its tasks, and Tokio cannot
  abort an already-running `spawn_blocking` closure.

No review agent was used. Task ownership, cancellation safety, durable-write
semantics, and process supervision formed one cross-crate chain that required a
single reviewer to keep the safe-to-drop versus must-complete distinction
consistent.

### Findings and decisions

- Added Ctrl-C/SIGTERM supervision to Polymarket, native MM, and L1 indexer.
- Fixed shared-MM shutdown during a stalled WebSocket handshake, retry sleep,
  read-only refresh, and simultaneously ready block event; added a stalled
  handshake regression test.
- Added safe cancellation points to Polymarket sync/feed/resolution work while
  preserving completion of already-started market/order/resolution writes.
- Made required Polymarket/native task exit a nonzero process failure, with a
  35-second internal deadline inside a 40-second Compose grace period.
- Made prover scheduler, source, and HTTP server one supervised process unit;
  child failure now stops siblings and permits `restart: on-failure`.
- Added L1 request deadlines and graceful monitoring shutdown, including fatal
  metrics-only mode.
- Replaced unbounded native verifier state/event root queues with bounded
  synchronous queues of 64.
- Opened [GitHub #184](https://github.com/MetaB0y/sybil/issues/184) for hard
  sequencer pre-mailbox backpressure and unambiguous write receipts and
  [GitHub #185](https://github.com/MetaB0y/sybil/issues/185) for process-level
  escalation after terminal canonical-owner restart failure. Both are Project
  1 Todo/Backlog/High.
- Opened [GitHub #186](https://github.com/MetaB0y/sybil/issues/186) for the
  deterministic lifecycle/signal/Loom test matrix and
  [GitHub #187](https://github.com/MetaB0y/sybil/issues/187) for explicit
  blocking/post-commit shutdown ownership. Both are Todo/Backlog/Medium.

### Result

- Report:
  [`code-quality-audit-actor-lifecycle-2026-07-17.md`](code-quality-audit-actor-lifecycle-2026-07-17.md)
- Changed-package tests, strict Clippy, formatting, verifier goldens,
  Compose/profile checks, and documentation gates pass.
- No deployment or validity/guest change occurred.
- The next cluster is error, panic, and recovery boundaries.

## 2026-07-17T20:22:29+01:00 — error, panic, retry, and recovery cluster

### Primary-reviewer method and reusable handoff

- Treat `unwrap`/`expect`/`panic` diagnostics as an inventory, not a defect
  count. Prove local invariant sites separately from environment, transport,
  persistence, and task-owner failures.
- Build durable-operation and retry matrices together. Atomic local publication
  does not make a preceding remote side effect retry-safe.
- A persisted resource may be replaced only after authoritative absence. A
  timeout, 5xx, decode failure, or authentication error says nothing about
  whether the resource exists.
- Retry remote creation only with a caller-stable intent/receipt. Persist the
  returned identity immediately after the response, before metadata or other
  fallible work.
- `rename` is an atomic visibility mechanism, not a complete power-loss
  durability claim. Sync file contents before publication and the containing
  directory after publication where the platform supports it.
- Keep availability and integrity separate: stable retryable 503 identities for
  actor/storage outages, fail-stop latches for invalid recovery.

No review agent was used. Durable canonical state, local operational files,
remote side effects, and HTTP/process error identity formed one failure chain
that needed a single classification owner.

### Findings and decisions

- Reviewed 139 diagnostic `unwrap`/`expect`/`panic` warning instances across
  selected production targets and local workspace dependencies; most
  sequencer/verifier sites were proved local fail-stop invariants rather than
  availability defects.
- Fixed native and Polymarket MM reattachment so only HTTP 404 permits a new
  funded account; corrupt state and transient failures now fail closed.
- Added deterministic Polymarket market creation keys, immediate durable
  mapping checkpoints, and recovery/adoption of a compatible group left by a
  crash before local checkpoint.
- Made the Polymarket mapping and API raw-event snapshots sync file contents,
  atomically publish, and sync the parent directory. Snapshot temp names also
  tolerate concurrent writers and stale prior-process files.
- Added stable non-leaking `503 SEQUENCER_UNAVAILABLE` and
  `503 SEQUENCER_PERSISTENCE_UNAVAILABLE` API errors.
- Replaced recoverable API startup panics/unwraps with contextual returned
  errors and made a configured event-snapshot directory fail closed.
- Made history signal registration fallible, supervised unexpected HTTP exit,
  and preserved projector shutdown before returning an error.
- Opened [GitHub #188](https://github.com/MetaB0y/sybil/issues/188) for
  idempotent funded-account provisioning (Project 1 Todo/Backlog/High).
- Opened [GitHub #189](https://github.com/MetaB0y/sybil/issues/189) for one
  crash-safe local-file primitive and fault matrix (Todo/Backlog/Medium).
- Updated existing [GitHub #129](https://github.com/MetaB0y/sybil/issues/129)
  with the Polymarket market-group crash witness; its protocol creation-key
  work remains open.

### Result

- Report:
  [`code-quality-audit-error-recovery-2026-07-17.md`](code-quality-audit-error-recovery-2026-07-17.md)
- Changed-package tests, focused raw-snapshot/OpenAPI integration tests, strict
  all-target/all-feature Clippy, Rust formatting, and `just docs-check` pass.
- No consensus bytes, guest/public-input behavior, or deployment pins changed.
- No deployment occurred.
- The next cluster is static lint, dead code, and unsafe-code policy.

## 2026-07-18T10:40:39+01:00 — static lint, dead code, and unsafe policy

### Primary-reviewer method and reusable handoff

- Render and compile isolated feature graphs; default plus all-features is not
  a feature-lattice audit.
- Treat dead public APIs as ownership questions. Delete only with a consumer
  search and a clear replacement, not merely because the compiler is quiet.
- Enforce a zero-unsafe repository at every Rust workspace boundary while it
  is still true.
- Require suppressions to state their invariant or tooling reason at the
  attribute site.

### Findings and decisions

- Found no authored unsafe construct across the root, fuzz, or OpenVM
  workspaces and converted that property into strict Clippy/CI gates.
- Isolated-feature Clippy exposed a no-solver `matching-sim` build failure and
  a feature-only dead benchmark helper; both were fixed.
- Made the `parallel` solver feature own the LP/decomposed code that consumes
  Rayon.
- Removed the unused `BatchSequencer` compatibility export.
- Documented retained lint suppressions outside the commitment-fingerprinted
  guest closure. The closure keeps its adjacent rationale and a narrow lint
  exception because even attribute-only edits require a real guest rebuild.
- Opened no issue because all accepted findings were bounded and remediated.

### Result

- Report:
  [`code-quality-audit-static-lint-unsafe-2026-07-18.md`](code-quality-audit-static-lint-unsafe-2026-07-18.md)
- Root all-feature, isolated-feature, and standalone-workspace strict Clippy
  form the completion gate.
- No proof generation, deployment, protocol-byte, or solver-policy change
  occurred.
- The next cluster is dependency and build supply chain.

## 2026-07-18T11:13:57+01:00 — dependency and build supply chain

### Primary-reviewer method and reusable handoff

- Audit lockfiles with current advisory databases, but keep the command outside
  deterministic compilation gates because the answer changes with time.
- Trace every advisory to its feature, build, host, guest, or runtime consumer
  before choosing update, removal, or an explicit exception.
- Prefer an upstream upgrade over a local vendor fork. An exception must name
  the unreachable affected API and have an issue that removes it.
- Keep emergency security releases able to bypass package-age quarantine by
  exact version only.

### Findings and decisions

- Removed fuzz's unsound dependency, two frontend Vite advisories, 40 Arena
  advisory records, and 36 visualization advisory records through lock
  refreshes. The unreachable SCIP and Ark R1CS advisories remain explicit.
- Added one current-advisory command covering all Cargo, pnpm, and uv locks.
- Pinned `cargo-chef` and the Arena uv build image instead of resolving
  `latest` during each Docker build.
- Retained and documented only the SCIP build-only `time`, Ark R1CS
  `tracing-subscriber`, and OpenVM proc-macro `lru::IterMut` exceptions; none
  of the affected APIs is called. A Commonware 2026.7 trial was rejected when
  the guest-fingerprint gate proved it required closure-source changes.
- Aligned the root and fuzz locks on Commonware 2026.5 and made the advisory
  gate assert that parity. The fingerprinted validity manifests remain
  untouched.
- Opened #194 for upstream removal and #195 for immutable Actions plus
  automated refresh. Existing #65 owns deployed image digests and #118 owns
  generated SDK packaging.

### Result

- Report:
  [`code-quality-audit-dependency-supply-chain-2026-07-18.md`](code-quality-audit-dependency-supply-chain-2026-07-18.md)
- Current RustSec, npm, and PyPA scans pass under the three named exceptions;
  root all-target/all-feature compilation, Arena's 318 tests, and frontend
  gates validate the refreshed graphs.
- No proof generation or deployment occurred.
- The next cluster is Python/Arena data and experiment correctness.
