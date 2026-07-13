---
adr: 0018
title: Extract private history behind a transactional outbox
status: Accepted
date: 2026-07-13
validity_critical: false
supersedes: [ADR-0017]
superseded_by: []
---

# ADR-0018 — Extract private history behind a transactional outbox

## Context

The sequencer previously constructed, indexed, pruned, and served fills,
account events, equity samples, prices, candles, and leaderboard baselines from
its own redb. Those rows are derived product data, not validity inputs. Their
write amplification extended the fenced block transaction; their range scans
ran through the single-writer actor; and their retention/index maintenance made
sequencer startup and publication depend on a growing historical workload.

The public block tape cannot rebuild private account history because it
intentionally omits account attribution. A synchronous write to an external
database would instead make network/database availability part of block
production. Product history and private recovery DA also have different
contents and security purposes.

## Decision

`matching-sequencer` writes one genesis-bound, versioned
`CommittedHistoryBatchV1` to a transactional outbox in the same redb commit
that flips the canonical state fence. It does no external I/O in that commit.
An out-of-actor API worker delivers contiguous batches at least once to the
private `sybil-history` service and deletes an outbox row only after the service
acknowledges a checkpoint covering it.

`sybil-history` atomically stores the immutable raw batch, updates all query
projections and candles, and advances a contiguous checkpoint. Exact duplicate
delivery is a no-op; gaps, parent/genesis mismatches, invalid canonical order,
and same-height conflicts fail closed. Its initial serving store is a separate
redb file. This is an implementation choice, not a promise that redb is the
permanent analytical store; the immutable batch contract is the migration seam
for PostgreSQL, object archive, or other projections later.

Historical REST handlers authenticate users in `sybil-api` and query the
private service with a dedicated history bearer token. They never expose the
internal service or raw account-attributed batches to browsers. Current
portfolio state remains a live sequencer read. Read-key ownership is copied
into an API-owned authorization view at startup and updated after acknowledged
key changes. Leaderboard bases are published into an API-owned view once per
committed block. Thus neither account-history authorization nor leaderboard
HTTP volume produces per-request sequencer mailbox traffic. Windowed baselines,
fills, events, equity series, prices, and candles are history-service reads.
Service failure returns an explicit history-unavailable response while
matching, admission, and block production continue and the durable outbox
accumulates.

Internal batch delivery uses authenticated MessagePack. Authentication runs
before body extraction, and the committed batch is the transport unit: there
is no arbitrary HTTP size rejection that could poison one height and block the
contiguous stream forever. Catch-up acknowledges a delivered prefix in one
sequencer-redb transaction rather than fsyncing once per height. History query
work has a hard process-local concurrency limit; queued or disconnected HTTP
requests cannot create unbounded blocking-reader tasks. Configured candle
resolutions are persisted and must match on reopen, so configuration drift
cannot silently create partial or stale series.

Canonical block/DA retention remains sequencer-owned and is not part of this
product-history service. History batches are private derived facts, not proof
inputs, escape material, or decentralized DA.

## Alternatives considered

- **Keep bounded history embedded** — operationally simple, but historical
  reads and maintenance still contend with the single-writer process and the
  arbitrary local window becomes a product limit.
- **Synchronous external dual writes** — zero visible lag, but an unavailable
  history database would stop canonical commits without a distributed
  transaction.
- **Project the public block stream** — correctly public, but deliberately
  lacks private account attribution needed by fills, events, and equity.
- **Share/tail the sequencer redb** — avoids delivery code but couples the new
  process to private table layout and file locking, and does not establish a
  replayable contract.
- **PostgreSQL/Kafka/ClickHouse immediately** — plausible future serving or
  analytical stores, but premature operational machinery for the current
  devnet. The batch boundary lets storage evolve without re-entering the
  sequencer.

## Consequences

**Good:** historical read volume, projection work, candles, account
authorization, and windowed leaderboards cannot occupy the sequencer actor per
request; startup does not scan product-history tables; history can retain beyond the former
30-day/row limits and evolve independently; committed facts survive consumer
outages and replay deterministically.

**Costs / constraints:** history is eventually indexed, so responses must
expose `indexed_through_height` and completeness; the service adds a process,
private credential, database, backup, and monitoring surface; the current
same-host deployment isolates load but not host failure; an unbounded outbox can
eventually exhaust sequencer disk during a prolonged outage and therefore needs
backlog alerts and an explicit production exhaustion policy.

**Follow-ups:** add an immutable off-host raw-batch archive before promising
network-lifetime preservation; choose partitioning/rollup and deletion policy
from measured rates; add checkpoint/backlog dashboards and restore drills; move
the serving projection to PostgreSQL or another store only when operational
evidence justifies it.
