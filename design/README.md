---
tags: [moc, guide]
layer: core
status: current
last_verified: 2026-07-17
---

# Design workspace

> **Executive summary:** only durable research and genuinely open proposals live
> here. Completed implementation plans, superseded architecture, dated roadmaps,
> reviews, and editorial material live under [`archive/`](archive/README.md).
> Nothing in `design/` overrides the current spec, ADRs, code, or tests.

## Status vocabulary

- **reference** — durable explanation or dated empirical result; not a backlog.
- **exploratory / proposed** — unratified possibility.
- **proposal-needs-revalidation** — potentially useful, but written against an
  older implementation and must be resurveyed before work begins.
- **current-audit** — dated findings that still need explicit remediation or
  closure; verify each finding against newer code before acting.
- **current** — only for an index that is actively maintained.

An accepted decision belongs in `docs/adr/`; implemented behavior belongs in
`docs/architecture/` and `docs/SPEC.md`. A completed plan belongs in the
archive.

## Foundations and reference

| Document | Status | Use |
|---|---|---|
| [`problem-statement.md`](problem-statement.md) | reference | Self-contained mathematical matching problem |
| [`math-papers.md`](math-papers.md) | current index | Canonical external proof-repository map |
| [`welfare-vs-volume.md`](welfare-vs-volume.md) | reference | Objective trade-off analysis |
| [`solver-benchmark-report-2026-07-13.md`](solver-benchmark-report-2026-07-13.md) | dated reference | Preregistered 675-run solver evaluation and operational recommendations |
| [`solver-benchmarks.md`](solver-benchmarks.md) | superseded dated reference | Earlier single-book results; retained to show research lineage, not for quotation |
| [`eg-conic.typ`](eg-conic.typ) | draft research | Quasi-linear EG conic reformulation |
| [`mint-pnl.typ`](mint-pnl.typ) | draft research | MINT-account accounting analysis |

## Open mechanism and product proposals

| Document | Status |
|---|---|
| [`conditional-combinatorial-markets.md`](conditional-combinatorial-markets.md) | exploratory |
| [`sealed-bid-batch-auctions.md`](sealed-bid-batch-auctions.md) | exploratory |
| [`data-availability-design.md`](data-availability-design.md) | proposed construction under ADR-0012 |
| [`trust-minimized-resolution.md`](trust-minimized-resolution.md) | exploratory |
| [`proof-of-reserves.md`](proof-of-reserves.md) | exploratory |
| [`capability-mask-keys.md`](capability-mask-keys.md) | exploratory; the committed field exists, scoped enforcement does not |
| [`user-cli-plan.md`](user-cli-plan.md) | proposed |
| [`epoch-prover-service.md`](epoch-prover-service.md) | proposed implementation plan under ADR-0019 |
| [`possibility-space-2026-07.md`](possibility-space-2026-07.md) | brainstorm, not backlog |

## Strategies requiring a fresh survey

These remain visible because their direction may still matter, but their
inventories, counts, and execution stages are not current:

- [`sybil-commitments-consolidation.md`](sybil-commitments-consolidation.md)
- [`testing-strategy-2026-07.md`](testing-strategy-2026-07.md)
- [`observability-otel-2026-07.md`](observability-otel-2026-07.md)
- [`bot-quality-plan.md`](bot-quality-plan.md)
- [`settlement-aggregation-swirl.md`](settlement-aggregation-swirl.md)

Resurvey the named code and issue state before turning any of these into work.

## Current audits

- [`code-quality-audit-session-report-2026-07-17.md`](code-quality-audit-session-report-2026-07-17.md)
  — detailed time, sequence, changes, evidence, issue, and handoff report for
  the six-cluster audit session.
- [`code-quality-audit-program-2026-07.md`](code-quality-audit-program-2026-07.md)
  — living cluster ledger, evidence hierarchy, and next-audit charter.
- [`code-quality-audit-exact-wire-2026-07-17.md`](code-quality-audit-exact-wire-2026-07-17.md)
  — Rust/OpenAPI/Python/TypeScript exact nanodollar contract; the broader
  64-bit policy remains open in GitHub #177.
- [`code-quality-audit-validity-mutation-2026-07-17.md`](code-quality-audit-validity-mutation-2026-07-17.md)
  — targeted validity-core mutation campaign, survivor ledger, verifier and
  settlement fixes, guest repin, and open policy issues #178/#179.
- [`code-quality-audit-economic-properties-2026-07-17.md`](code-quality-audit-economic-properties-2026-07-17.md)
  — independent settlement/MM/UCP oracles, complete-set mint/burn properties,
  structural certificate fix, and open reservation-model issue #180.
- [`code-quality-audit-api-client-conformance-2026-07-17.md`](code-quality-audit-api-client-conformance-2026-07-17.md)
  — OpenAPI inventory, generated-client parity, replay/retention safety fixes,
  and open executable-contract issues #181–#183.
- [`code-quality-audit-actor-lifecycle-2026-07-17.md`](code-quality-audit-actor-lifecycle-2026-07-17.md)
  — actor/task ownership matrix, cancellation and process-supervision fixes,
  bounded verifier workers, and open lifecycle work #184–#187.
- [`code-quality-audit-error-recovery-2026-07-17.md`](code-quality-audit-error-recovery-2026-07-17.md)
  — production panic/error classification, durable-operation and retry
  matrices, crash-window fixes, and open recovery work #188/#189.
- [`code-quality-audit-collaboration-log-2026-07.md`](code-quality-audit-collaboration-log-2026-07.md)
  — timestamped reviewer/agent handoff and decision record.
- [`dos-audit-2026-07-11.md`](dos-audit-2026-07-11.md) — permissionless
  resource/state-growth audit. Findings remain active until code or a dated
  follow-up closes them.

## Historical design lineage

- [`archive/implemented/`](archive/implemented/README.md) — plans/specs whose
  essential work landed (witness/key transitions, WebAuthn guest verification,
  escape claims, recovery).
- [`archive/superseded/`](archive/superseded/README.md) — architecture replaced
  by the canonical vault.
- [`archive/planning/`](archive/planning/README.md) — dated reviews, roadmaps,
  and execution-order snapshots.
- [`archive/review-2026-07-02/`](archive/review-2026-07-02/README.md) — original
  full-code audit.

For current behavior start with [`docs/README.md`](../docs/README.md) and
[`docs/SPEC.md`](../docs/SPEC.md).
