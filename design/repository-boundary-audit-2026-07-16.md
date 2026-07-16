---
tags: [planning, audit, architecture, simplification, codex]
status: unreviewed-codex-generated
date: 2026-07-16
---

# Sybil boundary audit after the simplification roadmap

Date: 2026-07-16  
Scope: merged `main` after PR #115  
Focus: simplicity, elegance, executable semantics, and single-owner boundaries

## Verdict

The first simplification audit was directionally correct and its highest-value
recommendations are now implemented: history queries belong to `sybil-history`,
production retained-cash no longer compiles the research solver surface, dead
prover/viz/OTLP paths are gone, first-party streaming is WebSocket-only, route
policy is declared once, Compose topology is explicit, Arena owns its database,
and native product markets no longer exist as side effects of the Polymarket
mirror.

The remaining accidental complexity is concentrated rather than systemic. The
clearest problems are stale persistence contracts, human-error parsing, missing
idempotency, an orphaned auto-resolution queue, canonical oracle states with no
executable policy, and a long-running native MM without an operational contract.
All are bounded changes with testable end states, so they should be implemented
before the next fresh-genesis deployment.

## Reconciliation with the first audit

The detailed HTML report is retained at
`design/repository-simplification-audit-2026-07-16.html`. Its recommendations now
stand as follows:

| Original recommendation | Current state |
|---|---|
| Remove sequencer history query caches | Complete |
| Isolate research solvers from production retained-cash | Complete |
| Delete the obsolete prover mock-live path | Complete |
| Make OpenTelemetry complete or absent | Complete: absent until deliberately funded |
| Remove stale solver-viz features/recipes | Complete |
| End Rust API reads of Arena SQLite | Complete |
| Separate native provisioning/resolution from Polymarket | Complete; follow-ups below |
| Retire first-party SSE | Complete |
| Remove non-executable oracle lifecycle states | Revalidated; #117 |
| Use one audited route registry | Complete |
| Make Compose topology explicit with profiles | Complete |
| Remove dead direct dependencies and centralize versions | Complete |
| Move generated Python SDK output out of review | Ambiguous packaging trade-off; #118 |

## High-confidence findings to implement now

1. **Polymarket mapping durability (#109).** The mirror owns one mapping file,
   but it still writes in place and infers chain identity from market probes.
   Give the file a schema version and genesis hash, save atomically, and rebuild
   deliberately on mismatch.

2. **Structured batch rejection (#110).** The shared MM parses a market id from
   API prose to recover from a resolved market in an atomic batch. Put the
   rejected market id in a stable error payload and remove the parser.

3. **Protocol idempotency for market creation (#111).** Native provisioning can
   recover from a crash only by matching titles/tags. A caller-supplied stable
   creation key should be committed with the market and conflicting reuse should
   fail. The scheduled genesis reset makes this the right time.

4. **Dormant native resolver schema (#112).** The catalog still validates and
   copies a future adapter enum after the resolver runtime was deleted. Keep the
   actual operator contract—resolution criteria and source URL—and delete the
   speculative adapter surface.

5. **Native MM operational contract (#113).** The process owns quoting, so it
   must also own readiness and progress/error metrics. Compose and Prometheus
   should observe it directly rather than infer health through another service.

6. **Shared MM configuration invariants (#114).** Validate the reusable policy
   once at startup for both processes. No non-finite or negative float may reach
   a float-to-protocol-integer conversion.

7. **Orphaned auto-resolution queue (#116).** Its only producer was removed in
   PR #115, but API routes, DTOs, actor messages, and a store table remain. Delete
   the unowned subsystem and regenerate clients.

8. **Executable canonical oracle state (#117).** Immediate signed resolution is
   the sole policy. Reduce canonical status to `Active | Resolved` and remove
   proposal/challenge/void payloads from witnesses, snapshots, state roots, and
   public DTOs. Richer policy should return only with defined transitions,
   economics, and conformance tests.

## Boundaries to keep

- Integer protocol truth versus floating-point solver/MM search.
- Single-writer sequencer actor and durable-before-live publication.
- History outbox/projector separation: projector outage must not block clearing.
- Native verifier semantics shared with a dependency-austere guest.
- Guest-safe L1 protocol hashes separated from host-only ABI/indexer code.
- Polymarket and native runtimes sharing quote mechanics but not lifecycle,
  persistence, accounts, or resolution authority.
- Independent research solvers as differential references outside production.

## Findings not implemented automatically

- **Generated Python SDK packaging (#118).** Removing tracked output requires a
  choice between an internal package and deterministic build-time generation.
  Both can be good; choosing without designing reproducibility is not a clear
  simplification.
- **Further splitting `sybil-market-maker`.** Quote policy and Sybil submission
  orchestration are coupled today, but both native and Polymarket need exactly
  that unit. Another abstraction would add seams without eliminating an owner.
- **Security/validity roadmap items.** TEE, real ZK/on-chain verification, escape
  maturity, and finalized Ethereum proof verification are important, but the
  requested private devnet explicitly does not depend on them. They remain
  roadmap work rather than being half-installed into the product path.

## Completion gate

Implement and merge #109–#117 except #118, run workspace/features/consensus/docs
and Compose gates, then deploy with every durable application volume cleared.
After genesis, verify service readiness, active quote loops, accepted orders,
trades/fills, fill ratio, history projection, and Prometheus/Grafana coverage.
