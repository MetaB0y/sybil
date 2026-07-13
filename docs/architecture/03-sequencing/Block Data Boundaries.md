---
tags: [infrastructure, analytics]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-07-13
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

Current aggregates such as 24h volume, trader counts, order statistics,
liquidity scores, and cost-basis summaries live in dedicated sequencer tracker
snapshots. They are derived from blocks, orders, fills, and account state; they
are not canonical block fields. Persistence for operational continuity does
not make them protocol truth.

Long-lived fills, account events, equity series, price history, candles, and
windowed leaderboard anchors belong to `sybil-history`. The sequencer exports a
small block-local fact batch through its fenced transactional outbox, but does
not own the serving indexes or execute historical scans. See [[Historical Data
Serving]].

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
