# `sybil-history-types`

Dependency-light boundary types shared by the sequencer outbox, API publisher,
and private history projector. It defines immutable block batches, neutral
historical facts, stable fact identities, and internal query/response shapes.

## Read first

- [[Historical Data Serving]]
- [[Block Data Boundaries]] and [[Persistence]]
- [ADR-0018](../../docs/adr/0018-extract-private-history-service.md)

## Boundaries

- Facts describe committed outcomes only; they must not expose live order-book
  state or become inputs to matching, settlement, roots, or proof validity.
- Batch identity is genesis-, height-, parent-, and payload-bound. Keep hashing
  deterministic and validate duplicate fact identities before persistence.
- Keep this crate independent of sequencer, API, storage, and transport
  implementations. Producers and consumers convert at their own boundaries.
- Account-attributed facts and queries are private internal data, even though
  these Rust types are reusable.
- Wire or hash changes require coordinated producer/consumer tests; early-dev
  breaking changes should still be explicit rather than silently permissive.
