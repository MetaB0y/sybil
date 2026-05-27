---
tags: [infrastructure, analytics]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-05-27
---

Sybil keeps canonical block data separate from derived product data. A
canonical `Block` is the exchange-facing record that can be replayed, hashed,
served over [[SSE Block Stream]], and related to a [[Block Witness]]. Derived
fields that are useful for API/frontend views live in a `BlockAnalytics`
sidecar and are transported with the block as `SealedBlock`.

## Canonical Block

`Block` contains data that describes what happened in the batch or anchors the
post-state:

- `BlockHeader`: height, parent hash, state root, events root, order/fill
  counts, timestamp
- accepted order ids
- system events
- bridge block data
- fills
- clearing prices
- rejected orders

These fields are the only block fields that should be treated as protocol
source-of-truth. They are the data model future witness/prover/on-chain paths
should reason about.

## Block Analytics Sidecar

`BlockAnalytics` is derived during block production and exists for product
views and observability:

- platform total welfare and volume
- orders filled
- unique placers
- per-market placers
- per-market volume
- per-market order placed/matched/unmatched counts
- per-market welfare

The sidecar is not part of the state root, events root, parent hash, or block
witness. It may be recomputed or reshaped without changing protocol semantics.
API and streaming responses may combine `Block` and `BlockAnalytics` into one
JSON response for client convenience, but the internal ownership boundary
remains explicit.

## Rolling Analytics

Rolling analytics such as 24h volume, price history, trader counts, order
statistics, liquidity scores, and cost-basis summaries live in dedicated
analytics trackers owned by the sequencer sidecar state. They are derived from
blocks, orders, fills, and account state; they are not canonical block fields.
Some of these trackers are persisted for operational continuity, but
persistence does not make them protocol source-of-truth.

Per-account equity series and account history feeds are store-backed product
analytics. New points/events are accumulated as a small block-local delta and
appended to redb at the same commit boundary as the block snapshot. The actor
keeps only a configurable in-memory serving fallback; production sets that
fallback to zero and serves these endpoints from redb. This prevents account UI
history from growing with total account cardinality in the hot sequencer heap.

## Indicative Cache

Indicative prices are off-block snapshots from a shadow solve over the resting
book. They are cached on the actor, not on `BlockSequencer`, and are served to
API clients as approximate open-batch data. The actor uses an in-flight guard
so the timer cannot stack multiple LP shadow solves under load. Freshness is
communicated by `computed_at_ms`.

## API Semantics

API models may intentionally join these layers:

- exact protocol values from `Block`
- per-block sidecar values from `BlockAnalytics`
- rolling persisted or in-memory analytics
- approximate indicative values from the actor cache

New API fields should document which category they come from and whether they
are exact, rolling-window, approximate, persisted, or since-restart.

## See Also

- [[Block Lifecycle]] — where blocks and witnesses are produced
- [[Block Witness]] — canonical audit trail for verification
- [[REST API]] — external response composition
- [[State Root and Parent Hash]] — canonical commitments
