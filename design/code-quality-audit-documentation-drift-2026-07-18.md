---
tags: [audit, code-quality, documentation, openapi, architecture]
layer: cross-cutting
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Documentation, API, and implementation-drift audit — 2026-07-18

## Result

The checked-in OpenAPI clients and frontend schema match the current API.
Three material documentation drifts were corrected: block-latency phase
ownership, the solver-fill account enrichment boundary, and a dated testing
proposal that still looked actionable despite completed work.

No runtime behavior changed solely to make documentation convenient.

## Scope

Reviewed:

- canonical route/OpenAPI generation and drift checks;
- handwritten and generated Python/TypeScript fill-history clients;
- deployment metric names versus the dashboard, alert rules, and latency
  runbook;
- current architecture notes touched by the L1, history, and settlement paths;
- vault freshness/link validation; and
- dated design/audit documents versus GitHub Issues as the current backlog.

## Findings

### DOC-1 — Block-latency semantics contradicted the metric producer

Severity: medium. Disposition: fixed with PERF-2.

The runbook called solver time a pipeline/storage measurement. It now defines
solve, prepare, persist, and successful production phases from their actual
recording sites, and the dashboard queries those exact names.

### DOC-2 — Settlement hid where account identity is attached to solver fills

Severity: low. Disposition: fixed.

The settlement note correctly said `settle_batch()` consumes
`fill.account_id`, but its statement that no `order_account_map` was needed
could be read as repository-wide. Solvers intentionally lack account context
and construct fills with sentinel id `0`; the sequencer uses its map at the
solver boundary to enrich each fill before settlement. The note now states both
owners and removes a duplicate `Market Resolution` link.

### DOC-3 — A historical testing proposal contained completed gaps and dated
counts

Severity: low. Disposition: fixed.

`design/testing-strategy-2026-07.md` was already marked
`proposal-needs-revalidation`, but its old inventory still named the shared
golden corpus, L1 tests, and Arena property tests as absent. Its status note now
explicitly says those items are implemented/superseded and directs readers to
the audit program and GitHub Issues for current disposition. The historical
analysis remains intact rather than being rewritten as if it were a live spec.

### DOC-4 — Generated interface drift is currently controlled

Severity: none. Disposition: retained.

The canonical OpenAPI document regenerated the Python SDK and TypeScript schema
without unexplained edits. The removed fill offset disappeared from both
generated surfaces, while unrelated market-search offsets remain. The
repository's route/schema/client checks are a stronger boundary than a manual
endpoint inventory and remain the completion gate.

## Verification

Passed:

- full OpenAPI rendering;
- Python SDK regeneration;
- frontend schema regeneration;
- generated drift checks;
- JSON validation for dashboards and golden vectors; and
- `just docs-check`, including vault/link/site validation.

No issue was opened because every accepted drift was fixed locally. Broader
cross-runtime WebSocket generation remains GitHub #183, and non-nanodollar
JavaScript integer policy remains #177.

## Residual risk

Historical files under `design/` will continue to age; their status and
supersession links, rather than continual historical rewrites, are the intended
control. Architecture notes still require owners to update `last_verified`
when behavior changes. No further contradiction found in this pass had a clear
implementation consequence.

