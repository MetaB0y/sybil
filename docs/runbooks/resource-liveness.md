---
tags: [runbook, observability, resources, data-availability]
status: current
last_verified: 2026-07-14
---

# Resource and DA liveness alerts

> **Executive summary:** these warnings catch persistent API-memory growth,
> host pressure/OOM kills, unusually fast permanent-state growth, large
> full-state witnesses, and a post-commit DA writer that is falling behind.
> Block commit remains authoritative, so preserve the store while separating
> process/host exhaustion from validity failures.

**Alerts:** `SybilApiMemoryGrowingFast`, `HostMemoryPressureStalled`,
`HostOomKill`, `SequencerAccountStockGrowingFast`, `WitnessPayloadLarge`, and
`DaArtifactWriteBacklog`.

## Signals and thresholds

| Alert | Threshold | Why it is conservative |
| --- | --- | --- |
| `SybilApiMemoryGrowingFast` | RSS slope above 32 MiB/hour for 30 minutes | Catches sustained retention well before the 650/800 MiB absolute alerts while ignoring short startup peaks. |
| `HostMemoryPressureStalled` | Full Linux PSI memory stalls above 5% for 5 minutes | By this point scrapes, DNS, and actor scheduling can already lag, so later queue values may be consequences rather than causes. |
| `HostOomKill` | Any kernel OOM kill in 10 minutes | Container state can remain stale when the host OOM killer terminates the payload process directly. |
| `SequencerAccountStockGrowingFast` | More than 100 committed accounts added in 15 minutes, sustained for 5 minutes | Accounts are permanent today and each account recurs in every later full-state witness. This is a traffic-anomaly warning, not a hard capacity limit. |
| `WitnessPayloadLarge` | Rolling p99 canonical witness payload above 8 MiB for 5 minutes | At the 10-second deployment cadence, retaining an 8 MiB payload every block is about 68 GiB/day before database overhead. The threshold is an early operating-cost warning, well below host-memory exhaustion. |
| `DaArtifactWriteBacklog` | More than two DA writes in flight for 2 minutes | One write per block is expected. Sustained overlap means the post-commit writer is not keeping pace and can consume memory through cloned witnesses and queued write tasks. |

`sybil_da_artifact_persist_duration_seconds` is supporting context. Compare its
rolling p99 with the configured block interval; persistence consistently near
or above that interval explains a growing in-flight count. DA payload, latency,
and in-flight metrics exist only when `SYBIL_DATA_DIR` enables the artifact
path. `sybil_state_accounts_total` is emitted on every committed block.

The in-flight gauge increments before each DA task is spawned and decrements
after its write result is recorded. A task panic deliberately leaves the gauge
high for that process lifetime, making the lost writer visible; restarting the
process resets the metrics recorder. Treat an unexplained stuck value as a task
failure and inspect logs before restarting.

## Triage

1. Confirm the API is still producing blocks and inspect
   `sybil_persistence_failures`, `sybil_da_artifact_persist_failures_total`,
   `sybil_process_resident_memory_bytes`, host free space, and container logs.
   For an OOM alert, use `journalctl -k` to distinguish a global host OOM from
   a cgroup kill; record the killed process's anonymous/file RSS and cgroup.
2. For RSS growth, correlate `sybil_recent_price_point_entries`,
   `sybil_recent_fill_entries`, `sybil_recent_equity_point_entries`,
   `sybil_recent_account_event_entries`, `sybil_recent_block_cache_len`,
   `sybil_product_history_outbox_backlog_rows`, witness size, and actor queue.
   A flat queue with growing cache entries is retention; a growing queue with
   flat caches is backpressure. Once PSI is high, treat queue growth as possibly
   downstream of host starvation.
3. For account growth, inspect request volume and callers for
   `POST /v1/accounts`. Confirm whether an onboarding event explains the rate;
   otherwise gate or rate-limit account creation before the stock grows further.
4. For a large witness, compare `sybil_state_accounts_total`,
   `sybil_pending_orders`, and `sybil_quarantine_ledger_size`. These are the
   principal recurring full-state populations currently exposed as metrics.
5. For a DA backlog, compare p99 DA persistence duration with block cadence and
   check redb volume latency/free space. A rising in-flight gauge with healthy
   block production means committed blocks may temporarily lack retained DA
   artifacts.
6. Verify a recent manifest and payload through the service-gated DA endpoints
   after the backlog clears. Investigate any increment in
   `sybil_da_artifact_persist_failures_total` even if the backlog gauge recovers.

Do not delete the store merely to clear these warnings. If retained history is
the pressure source, first preserve required artifacts and then apply the
configured block/DA retention policy. Account and live-order stock are current
state and are not repaired by history pruning.
