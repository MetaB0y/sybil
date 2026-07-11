---
tags: [moc, guide]
layer: core
status: current
last_verified: 2026-07-11
---

# Design workspace

> **Executive summary:** `design/` contains proofs, proposals, research, and
> planning inputs. It explains where Sybil may go, not necessarily what ships
> today. For current behavior start with [`docs/README.md`](../docs/README.md),
> [`docs/SPEC.md`](../docs/SPEC.md), and the implementation/tests.

## How to read a design document

Before implementing from anything here, check:

1. Is the proposal reflected in a current ADR?
2. Does the architecture note describe it as implemented or planned?
3. Do the named types, schemas, and tests still exist?
4. Has a newer document or code change superseded it?

Plans are allowed to become stale; current reference docs are not. When a plan
has served its purpose, move it to [`archive/`](archive/README.md) instead of
leaving it in the current documentation site.

## Foundations and durable research

- [`problem-statement.md`](problem-statement.md) — the matching problem Sybil is solving.
- [`math-papers.md`](math-papers.md) — index to the canonical Fisher-market proofs.
- [`welfare-vs-volume.md`](welfare-vs-volume.md) — why the objective is welfare rather than raw trade count.
- [`eg-conic.typ`](eg-conic.typ) and [`mint-pnl.typ`](mint-pnl.typ) — mathematical deep dives.
- [`solver-benchmarks.md`](solver-benchmarks.md) — benchmark methodology and dated results; rerun before quoting.

## Architecture and simplification

- [`architecture-review-2026-07.md`](architecture-review-2026-07.md) — simplification proposals; compare each proposal with current code before acting.
- [`general-advice-2026-07.md`](general-advice-2026-07.md) — cross-cutting engineering guidance.
- [`architecture-diagrams.md`](architecture-diagrams.md) — older diagram collection; current diagrams live in `docs/architecture/`.
- [`sybil-commitments-consolidation.md`](sybil-commitments-consolidation.md) — canonical-encoding consolidation proposal.
- [`testing-strategy-2026-07.md`](testing-strategy-2026-07.md) and [`observability-otel-2026-07.md`](observability-otel-2026-07.md) — quality/observability strategy.

## Custody, validity, and data availability

- [`keys-and-escape-ratification.md`](keys-and-escape-ratification.md) — consolidated custody decisions; ADRs own accepted outcomes.
- [`escape-claim-plan.md`](escape-claim-plan.md), [`escape-claim-guest.md`](escape-claim-guest.md), and [`escape-hatch-reconstruction.md`](escape-hatch-reconstruction.md) — escape/recovery design lineage.
- [`account-keys-digest.md`](account-keys-digest.md), [`openvm-p256-integration.md`](openvm-p256-integration.md), and [`capability-mask-keys.md`](capability-mask-keys.md) — authorization designs.
- [`data-availability-design.md`](data-availability-design.md) and [`proof-of-reserves.md`](proof-of-reserves.md) — availability and solvency proposals.
- [`witness-schema-v2.md`](witness-schema-v2.md) and [`witness-v6-keys-transition.md`](witness-v6-keys-transition.md) — historical schema inputs; the current witness is in [`docs/SPEC.md`](../docs/SPEC.md#6-blocks-state-and-witness-v9).

## Future product and mechanism space

- [`possibility-space-2026-07.md`](possibility-space-2026-07.md) — scored possibility map.
- [`conditional-combinatorial-markets.md`](conditional-combinatorial-markets.md) — conditional/bundle-market direction.
- [`sealed-bid-batch-auctions.md`](sealed-bid-batch-auctions.md) — private-order-flow exploration.
- [`trust-minimized-resolution.md`](trust-minimized-resolution.md) — challenge-based resolution proposal.
- [`user-cli-plan.md`](user-cli-plan.md) and [`bot-quality-plan.md`](bot-quality-plan.md) — client/agent plans.

## Time-sensitive planning

Files named `roadmap-*`, `execution-order-*`, or `*-plan` are dated coordination
artifacts. Their ordering and status can expire quickly. Use the issue tracker
and current code for live execution state; archive a file when it stops being
useful.

## Archive

[`archive/`](archive/README.md) preserves old audits, editorial drafts,
applications, pitches, and strategy snapshots. It is intentionally outside the
built documentation site and must never be cited as current implementation
truth.
