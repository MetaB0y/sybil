---
tags: [moc, guide]
layer: core
status: current
last_verified: 2026-07-07
---

# `design/` — specs, proofs, and where things are going

This tree answers **"where is it going?"** — forward specs, mathematical
foundations, and design explorations. For **"how does it work today?"** see
[`../docs/architecture/`](../docs/architecture/Sybil%20Architecture.md); for
**"why is it this way?"** see [`../docs/adr/`](../docs/adr/); for the honest
current state see [`../docs/review/`](../docs/review/00-executive-summary.md).

Status markers: **proof** (math) · **spec** (ratified/near-ratified) ·
**strategy** (how-to program) · **exploratory** (design sketch) · **review**.

## Foundations — the math
The theoretical core: why the matching problem is what it is.
- [`problem-statement.md`](problem-statement.md) — what Sybil is solving.
- [`lmsr-proof.typ`](lmsr-proof.typ) — **proof** · prediction markets are Fisher markets; the theoretical spine ([ADR-0001](../docs/adr/0001-eg-fisher-market-matching.md)).
- [`math-primer.typ`](math-primer.typ) — the arithmetic/convex-analysis background.
- [`welfare-vs-volume.md`](welfare-vs-volume.md) — why we maximize welfare, not trades.
- [`decomposition.typ`](decomposition.typ) / [`bundle-clearing.typ`](bundle-clearing.typ) — **proof** · combinatorial/bundle clearing (the substrate for conditional markets).
- [`eg-conic.typ`](eg-conic.typ) — Eisenberg–Gale ↔ conic formulations.
- [`mint-pnl.typ`](mint-pnl.typ) — minting and P&L accounting.
- [`solver-benchmarks.md`](solver-benchmarks.md) — solver performance data.

## Architecture — the big picture
- [`architecture-review-2026-07.md`](architecture-review-2026-07.md) — **review** · the Philosophy-of-Software-Design pass over the whole system (the canonical PoSD analysis — start here for boundaries/complexity).
- [`general-advice-2026-07.md`](general-advice-2026-07.md) — cross-cutting engineering guidance + open items.
- [`architecture-diagrams.md`](architecture-diagrams.md) — the diagram collection (rendered by `preview.html`).
- *(`architecture.md` is superseded by [Sybil Architecture](../docs/architecture/Sybil%20Architecture.md).)*

## Consensus & custody — the D-cluster
The proven-key / escape / redeploy work that moves the guest commitment.
- [`account-keys-digest.md`](account-keys-digest.md) — **spec** · `keys_digest` in account leaves (SYB-225).
- [`escape-claim-guest.md`](escape-claim-guest.md) — **spec** · the cash-escape guest (SYB-32).
- [`escape-hatch-reconstruction.md`](escape-hatch-reconstruction.md) — state reconstruction for escape/replacement (SYB-80).
- [`keys-and-escape-ratification.md`](keys-and-escape-ratification.md) — **spec** · the D0–D10 decisions consolidated for ratification.
- [`capability-mask-keys.md`](capability-mask-keys.md) — **exploratory** · scoped delegated authority (trade-not-withdraw keys); refines D1 — reserve the byte slot in v4 now.
- [`openvm-p256-integration.md`](openvm-p256-integration.md) — **spec** · in-guest P-256 ECDSA recipe ([ADR-0008](../docs/adr/0008-in-guest-p256-openvm-ecc.md)).
- [`witness-schema-v2.md`](witness-schema-v2.md) — the canonical witness format design (v2→v3 precedent).

## Quality & operations — strategy
- [`testing-strategy-2026-07.md`](testing-strategy-2026-07.md) — **strategy** · bulletproof testing (defense against divergence; the P0 gaps).
- [`observability-otel-2026-07.md`](observability-otel-2026-07.md) — **strategy** · OpenTelemetry/OTLP tracing spine.

## Crispness — buildable refactors
- [`sybil-commitments-consolidation.md`](sybil-commitments-consolidation.md) — **strategy** · one home for every canonical encoding.
- *(God-module decomposition lives in [`../docs/review/god-module-decomposition.md`](../docs/review/god-module-decomposition.md).)*

## Forward — the possibility space
Where the product could go once the trust story is solid.
- [`possibility-space-2026-07.md`](possibility-space-2026-07.md) — 17 future directions, scored; the map of the after.
- [`sealed-bid-batch-auctions.md`](sealed-bid-batch-auctions.md) — **exploratory** · MEV-resistance as a structural property.
- [`conditional-combinatorial-markets.md`](conditional-combinatorial-markets.md) — **exploratory** · the Fisher-market headline capability.

## Tooling
- [`user-cli-plan.md`](user-cli-plan.md) — the custody/user CLI plan.
