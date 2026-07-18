# `matching-sequencer`

Deterministic single-writer exchange state and persistence. `BlockSequencer` is
the synchronous core; a supervised actor exposes it through `SequencerHandle`.

## Read first

- [[Block Lifecycle]], [[Order Admission]], and [[Pending Orders and TTL]]
- [[Settlement]], [[Persistence]], and [[Acknowledged-Write WAL Replay]]
- [[Block Witness]] and [[State Root Schema]]

## Load-bearing rules

- Prepare on a clone; persist the block and flip the redb commit fence before
  swapping live state or publishing.
- Every acknowledged between-block mutation is durable-before-live in one
  globally ordered WAL. Recovery requires and replays the complete interval.
- redb is commit authority; recovery reads only the fenced qMDB slot.
- Witness, proof job, and pre/post qMDB proofs cross the same fence before
  qMDB A/B rotation.
- Resting-order reservations are owned by `OrderBook`; replay re-derives
  aggregates.
- Settlement uses shared integer helpers and MINT derivation.
- Unsupported value-bearing order shapes fail closed at admission.
- Canonical roots cover all typed state, including lifecycle, bridge,
  reservations, counters, withdrawals, and claims. History/analytics never
  become validity inputs.

Persistence changes require crash, restart, import, and API-integration
coverage in addition to the crate tests.
