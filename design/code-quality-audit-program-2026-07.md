---
tags: [audit, code-quality, planning, testing, codex]
layer: cross-cutting
status: current
date: 2026-07-17
last_verified: 2026-07-18
---

# Code-quality audit program — July 2026

This is the living cluster index for repository-wide code-quality review. Each
cluster gets its own dated report, executable evidence, and remediation or
tracked issues. It is not an architecture source of truth and does not replace
crate `AGENTS.md` files, the architecture vault, ADRs, tests, or GitHub Issues.

## Operating model

For one cluster at a time:

1. Research applicable standards, empirical review techniques, and mature
   tooling.
2. Read repository and crate guidance plus the relevant architecture notes.
3. State the scope, evidence boundary, severity model, and completion gate.
4. Build a semantic inventory and trace feasible paths before accepting an AI
   finding.
5. Require an executable witness where practical: a failing test, minimal
   reproducer, static-tool trace, generated-schema mismatch, differential
   result, or benchmark.
6. Fix obvious bounded defects in the same cluster and add regression coverage.
7. Deduplicate broader work against GitHub Issues, then create/update a
   repository issue with acceptance criteria and Project 1 metadata.
8. Run proportionate gates and record both passes and pre-existing blockers.
9. Publish a comprehensive dated report and update the collaboration log.

Review agents are used sparingly. They may inventory an independent slice or
challenge the method, but the primary reviewer validates their claims against
code and tests. Parallelism is not a substitute for one coherent owner.

## Evidence hierarchy

From strongest to weakest for this repository:

1. Cross-implementation or cross-runtime equality against an independent
   oracle.
2. Property/stateful tests generated from protocol invariants and schemas.
3. Deterministic known-answer vectors for hashes, signatures, encodings,
   settlement, and public inputs.
4. Static data-flow findings with a feasible path and local reproducer.
5. Mutation results that demonstrate whether an existing oracle notices a
   behavior change.
6. Linter/static-analysis warnings, reviewed for repository context.
7. Model-only suggestions without a witness.

Items in the last category are leads, not findings.

## Research basis

The program is deliberately repository-aware and validation-heavy:

- [GitHub's code-review customization
  guidance](https://docs.github.com/en/copilot/tutorials/customize-code-review)
  recommends concise, path-specific instructions and human validation.
- [RepoAudit](https://arxiv.org/abs/2501.18160) provides evidence for combining
  repository reasoning with validator-backed feasible paths.
- [Schemathesis](https://arxiv.org/abs/2112.10328) and
  [RESTler](https://arxiv.org/abs/1806.09739) support schema-derived boundary
  generation and dependency-aware request sequences.
- [cargo-mutants](https://mutants.rs/) provides a practical Rust mutation
  runner, but mutation score is treated as diagnostic rather than a quality
  target.
- Rust's maintained lint baseline remains
  [Clippy](https://doc.rust-lang.org/clippy/); Python and frontend findings use
  the repository-pinned Ruff, pytest, TypeScript, ESLint, and Vitest gates.

## Cluster ledger

| Cluster | State | Durable artifact / next action |
|---|---|---|
| Permissionless resource and state growth | Active remediation | [`dos-audit-2026-07-11.md`](dos-audit-2026-07-11.md); open economics/retention work remains |
| Repository ownership and accidental complexity | Prior review, partially reconciled | [`repository-boundary-audit-2026-07-16.md`](repository-boundary-audit-2026-07-16.md) and follow-up |
| Cross-language exact nanodollar wire fidelity | Audited and fixed; broader integer policy open | [`code-quality-audit-exact-wire-2026-07-17.md`](code-quality-audit-exact-wire-2026-07-17.md), GitHub #177 |
| Test-oracle effectiveness in the validity core | Audited and strengthened; two policy issues open | [`code-quality-audit-validity-mutation-2026-07-17.md`](code-quality-audit-validity-mutation-2026-07-17.md), GitHub #178 and #179 |
| Adversarial economic and mechanism properties | Audited and strengthened; temporal reservation model open | [`code-quality-audit-economic-properties-2026-07-17.md`](code-quality-audit-economic-properties-2026-07-17.md), GitHub #180 |
| Stateful API and generated-client conformance | Audited and strengthened; executable rejection/state/message contracts open | [`code-quality-audit-api-client-conformance-2026-07-17.md`](code-quality-audit-api-client-conformance-2026-07-17.md), GitHub #181–#183 |
| Actor lifecycle, cancellation, and supervision | Audited and strengthened; sequencer admission/escalation/blocking ownership open | [`code-quality-audit-actor-lifecycle-2026-07-17.md`](code-quality-audit-actor-lifecycle-2026-07-17.md), GitHub #184–#187 |
| Error, panic, and recovery boundaries | Audited and strengthened; service-create identity and file fault matrix open | [`code-quality-audit-error-recovery-2026-07-17.md`](code-quality-audit-error-recovery-2026-07-17.md), GitHub #188/#189 and existing #129 |
| Static lint, dead code, and unsafe-code policy | Audited, fixed, and enforced | [`code-quality-audit-static-lint-unsafe-2026-07-18.md`](code-quality-audit-static-lint-unsafe-2026-07-18.md); zero authored unsafe and strict feature/standalone gates |
| Dependency and build supply chain | Audited and remediated; three upstream exceptions open | [`code-quality-audit-dependency-supply-chain-2026-07-18.md`](code-quality-audit-dependency-supply-chain-2026-07-18.md), GitHub #194/#195 plus existing #65/#118 |
| Performance and algorithmic complexity | Queued | Allocation/copy hotspots, serialization amplification, non-solver worst cases, query indexes, benchmark regression gates |
| Documentation, API, and implementation drift | Queued | Route/OpenAPI pins, generated clients, config/runbook drift, architecture freshness and link gates |
| Python/Arena data and experiment correctness | Audited and fixed | [`code-quality-audit-arena-correctness-2026-07-18.md`](code-quality-audit-arena-correctness-2026-07-18.md), GitHub #192/#193 |
| Frontend semantic correctness and accessibility | **Next** | Exact-domain conversions, cache invalidation, async races, keyboard/screen-reader flows, failure-state UX |
| Solidity/L1 differential semantics | Queued | Rust/Solidity hash/ABI parity, invariant tests, reorg/finality assumptions, mutation/fuzz campaigns |

## Next-cluster charter: frontend semantic correctness and accessibility

The next cluster should answer one question: does the browser present exact,
current exchange state through accessible interactions without inventing
success, hiding failure, or racing realtime and backfilled data?

Initial scope:

- exact nanodollar/share conversion and formatting at every component edge;
- query-cache ownership, invalidation, and current-chain identity;
- WebSocket replay/backfill handoff, stale loading labels, and async races;
- mutation success/error/unknown states and optimistic UI rollback;
- keyboard, focus, dialog, form-label, reduced-motion, and screen-reader flows;
- chart scale/smoothing semantics and small-card information integrity; and
- natural-language computer-use scenarios alongside focused Vitest/component
  evidence.

Method:

1. Trace each page from generated schema through parsing, cache, provider, and
   rendered state before interpreting a visual symptom.
2. Use exact boundary corpora and fake-clock/deferred-promise tests for races.
3. Exercise primary flows with keyboard-only and semantic DOM assertions.
4. Treat “loading,” “empty,” “stale,” “failed,” and “ready” as distinct states.
5. Keep chart transforms visually helpful but semantically labelled and
   reversible; never smooth values used for settlement or exact display.
6. Fix bounded findings and file only coherent product/architecture trade-offs
   after deduplicating GitHub Issues.

Completion requires a dated page/data-flow inventory, executable exactness and
race evidence, primary accessibility coverage, reviewed chart semantics,
natural-language computer-use scenarios, generated-client drift checks, full
frontend gates, deduplicated issues, and explicit unresolved product choices.

## Reusable review prompt

The following compact brief should be adapted to the crate rather than copied
without context:

> Read all applicable repository/crate instructions and named architecture
> notes. Audit only the stated cluster. Trace feasible call/data paths and
> distinguish public, service, dev, research, and guest code. Accept a finding
> only with an executable witness or precise path plus violated invariant.
> Report severity, preconditions, blast radius, minimal reproduction, and the
> narrowest regression test. Preserve integer protocol truth and existing
> single-owner boundaries. Do not edit outside the cluster, do not create
> duplicate issues, and do not treat generated code or model confidence as
> proof.

## Reporting requirements

Every cluster report records:

- date, scope, evidence boundary, and architecture sources read;
- research sources and how they changed the method;
- findings with stable local IDs, severity, evidence, and disposition;
- exact changes and deliberately deferred work;
- tests, lints, generators, benchmarks, and any blocked gates;
- GitHub issue numbers and project state;
- completion criteria and residual risk.

The timestamped handoff record is
[`code-quality-audit-collaboration-log-2026-07.md`](code-quality-audit-collaboration-log-2026-07.md).
