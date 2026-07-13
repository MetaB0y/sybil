---
tags: [infrastructure, storage]
layer: sequencer
crate: sybil-history
status: current
last_verified: 2026-07-13
---

# Fill history persistence

`GET /v1/accounts/{id}/fills` is a private owner-facing audit/product endpoint.
Balances and positions are canonical current state; fill rows are durable
derived facts that explain how that state changed.

## Data model and commit boundary

The sequencer derives `AccountFillRecord` during committed block production.
The record contains the order id, fill quantity/price, block height, timestamp,
and signed per-market/outcome position deltas. Before the fenced redb commit it
converts the current block delta to `AccountFillFact` inside a
`CommittedHistoryBatchV1`.

The batch outbox row is written in the same transaction as the canonical state
fence. Therefore a committed block always has a replayable fill export and an
uncommitted candidate never leaks one. The sequencer does not write the serving
index, call the history process, or wait for projection.

`sybil-history` applies the raw batch and its fill projection atomically. The
key is ordered by `(account_id, block_height, order_id)`, making the public
`"<block_height>.<order_id>"` cursor stable. Exact batch redelivery is a no-op;
same-height content conflicts fail closed.

## Serving

`sybil-api` first authorizes the account owner, then sends an account-scoped
query to the private history service with a dedicated service credential.
Forward cursor pages are ascending; default/offset pages remain newest-first
for compatibility. Market filtering checks the fill's position deltas.

The response exposes the projector's `indexed_through_height` and
`history_complete_from_height`. History service failure is a typed unavailable
response, not an empty page and not a reason to stop trading. The API does not
merge in-memory sequencer records with remote pages because dual-source cursor
semantics are unsafe.

Sequencer restart restores canonical counters and aggregate snapshots, not
fill rows. The recent fill recorder may be empty after restart without affecting
the durable history endpoint or the canonical all-time fill count.

## Privacy and authority

Account-attributed fills are not on the public market tape and the internal raw
batch/queries are not browser surfaces. Fill history is not a state-root input,
proof input, balance authority, recovery witness, or DA substitute. Deleting or
rebuilding the projection cannot change exchange state.

## See also

- [[Historical Data Serving]]
- [[Persistence]]
- [[Settlement]]
- [[REST API]]
- [ADR-0018](../../adr/0018-extract-private-history-service.md)
