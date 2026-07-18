---
tags: [audit, code-quality, performance, history, observability]
layer: cross-cutting
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Performance and algorithmic-complexity audit — 2026-07-18

## Result

The audited non-solver hot paths no longer expose offset-based fill-history
scans, and block production now reports solve, complete prepare, persistence,
and successful end-to-end actor durations separately. A wall-clock race in a
sequencer test was also removed.

This pass deliberately did not alter solver policy, block cadence, canonical
commit ownership, or history retention. No deployment or proving occurred.

## Scope and method

Traced:

- block preparation, clone/discard, persistence, publication, and metrics;
- history table key order, reverse/forward pagination, market filtering,
  equity downsampling, baseline lookup, candle queries, and page bounds;
- account-fill REST, Rust DTO, history service, generated Python SDK, Arena
  reconciliation, and frontend consumers;
- serialization/code-generation amplification at that boundary;
- existing load/isolation checks and solver versus non-solver measurement; and
- the existing product-history backlog issue.

A suspected hotspot was accepted only when its cost could be derived from the
actual loop/index or reproduced by a failing test. No arbitrary benchmark
threshold was invented without a representative workload.

## Findings

### PERF-1 — Deprecated fill offsets caused work proportional to skipped history

Severity: medium. Disposition: fixed.

The default fill query iterated newest-first, decoded every market-matching row,
and discarded `offset` rows before collecting a bounded page. Large offsets
therefore occupied a bounded history worker with unbounded linear work while
also having unstable pagination semantics as new rows arrived.

The deprecated offset contract was removed end to end:

- `FillQuery`, REST parameters, generated clients, Arena, and the browser hook
  now expose newest-page or stable cursor semantics only;
- the default read stops after the bounded newest page;
- forward continuation remains keyed by `(block_height, order_id)`; and
- the API rejects the removed `offset` parameter rather than silently ignoring
  it.

Market search still has a separate bounded offset contract; it was not changed
under the account-history finding.

### PERF-2 — The latency runbook attributed persistence stalls to solver time

Severity: medium. Disposition: fixed.

`sybil_solve_time_seconds` is populated from
`PipelineResult.total_time_secs`; it measures solver work, not complete block
production. The runbook nevertheless described it as including storage and
recommended diagnosing redb stalls from that metric. Operators could therefore
see a persistence stall with a flat solve graph and pursue the wrong subsystem.

The actor now records:

- `sybil_block_prepare_duration_seconds`;
- `sybil_block_persist_duration_seconds`; and
- `sybil_block_production_duration_seconds`.

Persistence duration is recorded for failed attempts as well as successes.
Complete production is recorded only for a committed tick. The dashboard shows
the three phases beside solve percentiles, and the runbook now states the
boundaries exactly. Existing solve alerts were not silently repurposed.

No threshold alert was added for the new phases: choosing one before observing
the deployed cadence and workload distribution would be arbitrary. The metrics
make that later decision evidence-based.

### PERF-3 — A manually driven actor test still allowed the real scheduler to
fire

Severity: low. Disposition: fixed.

The history-outbox test manually requested its blocks but configured a
60-second scheduler. Under a contended full suite, the scheduler could inject
an unrelated extra block and make an exact-height assertion flaky. The test now
uses a one-hour interval and documents that block production is manual. The
formerly flaky test passes independently.

### PERF-4 — Reviewed storage loops are bounded or index-aligned

Severity: none. Disposition: retained.

- Fill pages use the `(account, height, order)` key and stop at a capped page.
- Event, price, and candle reads use ordered bounds and bounded result
  retention.
- Equity reads scan the selected account/time window because a truthful series
  needs those samples, but compact during the scan and cap the response at
  5,000 points.
- The prepare-time sequencer clone is the mechanism that permits discard on
  persistence failure before the canonical in-memory swap. Replacing it with
  partial mutation would trade visible copying for rollback complexity.
- Market-filtered account fills may inspect more account rows because one fill
  can carry multiple position deltas. A second projection could improve a
  specific workload but adds write/storage ownership; current evidence does
  not establish that trade as a net win.

### PERF-5 — History backlog capacity remains a deliberate architecture choice

Severity: high, pre-existing. Disposition: GitHub #90.

Projector outage can grow the canonical-store outbox. Silently deleting rows or
halting block production at an improvised threshold changes the completeness or
liveness contract. GitHub #90 already owns measurement, policy, ADR, telemetry,
and failure-state acceptance criteria, so this audit did not create a duplicate
or choose an ad hoc limit.

## Verification

Passed:

- history and history-types package tests;
- cursor/default/rejected-offset API integration coverage;
- the formerly flaky sequencer outbox regression;
- Arena fill-reconciliation tests and Ruff;
- regenerated Python and TypeScript clients;
- frontend TypeScript and all 384 passing Vitest tests (one skipped);
- golden/config JSON parsing; and
- monitoring configuration/rule checks.

No solver benchmark was rerun because this cluster made no solver change.

## Residual risk

The new phase metrics need deployment observations before meaningful SLOs or
benchmark regression thresholds can be set. Indexing account fills by market
and changing equity retention are workload/product decisions. The outbox bound
is owned by #90. Those are the remaining performance leads; none is an
unambiguous simplification without additional evidence.

