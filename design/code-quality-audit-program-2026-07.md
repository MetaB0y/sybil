---
tags: [audit, code-quality, planning, testing, codex]
layer: cross-cutting
status: current
date: 2026-07-17
last_verified: 2026-07-17
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
| Dependency and build supply chain | **Next** | Advisories, license/source policy, pinned tools, generated artifacts, lockfiles, reproducibility, feature unification |
| Performance and algorithmic complexity | Partially covered by DoS audit | Allocation/copy hotspots, serialization amplification, solver worst cases, query indexes, benchmark regression gates |
| Documentation, API, and implementation drift | Partially covered | Route/OpenAPI pins, generated clients, config/runbook drift, architecture freshness and link gates |
| Python/Arena data and experiment correctness | Queued | Time semantics, leakage, determinism, fixture realism, result persistence, bot isolation |
| Frontend semantic correctness and accessibility | Queued | Exact-domain conversions, cache invalidation, async races, keyboard/screen-reader flows, failure-state UX |
| Solidity/L1 differential semantics | Queued | Rust/Solidity hash/ABI parity, invariant tests, reorg/finality assumptions, mutation/fuzz campaigns |

## Next-cluster charter: static lint, dead code, and unsafe-code policy

The next cluster should answer one question: can automated static checks reject
new production hazards without hiding feature-specific code, generated
artifacts, deliberate invariant assertions, or guest constraints in a noisy
workspace-wide allowlist?

Initial scope:

- normal, all-target, and all-feature Clippy surfaces across every workspace;
- crate/module lint overrides and suppressed warnings, including the reason and
  narrowest scope for each;
- exported-but-unused APIs, feature-only helpers, obsolete compatibility
  branches, and generated-code boundaries;
- every `unsafe` block/function/impl plus the invariant that makes it sound;
- production `unwrap`/`expect`/`panic` policy informed by the completed
  error/recovery inventory; and
- CI/toolchain differences that cause local strict lint to diverge from the
  checked gate.

Method:

1. Render the effective feature/target matrix before interpreting warnings.
2. Capture a reproducible baseline by crate, lint, target, and feature profile.
3. Prove reachability and ownership before deleting an export or branch.
4. Require a local safety comment and executable invariant evidence for each
   retained `unsafe` boundary.
5. Convert reviewed production panic restrictions into scoped denies plus
   explicit, reasoned allows; do not enable restriction lints indiscriminately.
6. Fix bounded findings and file only coherent policy/architecture work after
   deduplicating GitHub Issues.

Completion requires a dated lint/unsafe inventory, feature-matrix evidence,
zero unexplained strict warnings in the selected production profiles, bounded
dead-code cleanup, reviewed unsafe invariants, documented suppressions,
deduplicated issues, and proportionate test/documentation gates.

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
