# `sybil-history`

Private derived-history projector and query service. It consumes immutable,
genesis-bound batches from the sequencer's transactional outbox and owns raw
history retention, account/time indexes, candles, pagination, and historical
query load.

## Read first

- [[Historical Data Serving]]
- [[Block Lifecycle]] and [[Persistence]]
- [[REST API]] and [[Block Data Boundaries]]

## Load-bearing rules

- The outbox is durable truth; actor messages and HTTP delivery are hints.
- Apply a raw batch, every projection row, candles, and the contiguous
  checkpoint in one transaction.
- Duplicate delivery is a verified no-op; gaps, genesis changes, parent-hash
  mismatches, and same-height/different-payload deliveries fail closed.
- Queries read the projection directly and never traverse the ingestion actor
  or sequencer actor.
- History is derived and must never feed matching, settlement, roots, or proof
  validity.
- Account-attributed ingestion and queries are private internal surfaces.

Run `cargo test -p sybil-history` after changing ingestion or query semantics.

