---
tags: [runbook, observability, resources, data-availability]
status: current
last_verified: 2026-07-21
---

# Resource and DA liveness alerts

> **Executive summary:** these warnings catch persistent API-memory growth,
> measured web/history memory-envelope breaches, host pressure/OOM kills,
> root-filesystem exhaustion, unusually fast permanent-state growth, a
> stale/large product-history source backlog, large full-state witnesses, and
> a post-commit DA writer that is falling behind.
> Block commit remains authoritative, so preserve the store while separating
> process/host exhaustion from validity failures.

**Alerts:** `SybilApiMemoryGrowingFast`, `HostMemoryPressureStalled`,
`HostOomKill`, `ContainerRestartObserved`, `ContainerOomKilled`,
`MeasuredMemoryEnvelopeExceeded`, `ContainerMemoryHigh`,
`ContainerMemoryCritical`,
`HostDiskSpaceLow`, `HostDiskSpaceCritical`,
`SequencerAccountStockGrowingFast`,
`PublicAccountCapacityLow`, `PublicAccountCapacityExhausted`,
`ProductHistoryOutboxStale`, `ProductHistoryOutboxLarge`,
`WitnessPayloadLarge`, and `DaArtifactWriteBacklog`.

## Signals and thresholds

| Alert | Threshold | Why it is conservative |
| --- | --- | --- |
| `SybilApiMemoryGrowingFast` | After the recent-block cache has been size-stable across the complete 30-minute slope window, RSS slope above 32 MiB/hour for 30 minutes | Catches sustained post-warm-up retention well before the 650/800 MiB absolute alerts. The absolute alerts remain active during startup. |
| `HostMemoryPressureStalled` | Full Linux PSI memory stalls above 5% for 5 minutes | By this point scrapes, DNS, and actor scheduling can already lag, so later queue values may be consequences rather than causes. |
| `HostOomKill` | Any kernel OOM kill in 10 minutes | Container state can remain stale when the host OOM killer terminates the payload process directly. |
| `ContainerRestartObserved` | A Compose service's Docker restart count increases or its started-at timestamp changes within 10 minutes | A replacement gets a new container whose restart count begins at zero, so the retained start timestamp covers both automatic restarts and whole-container recreation. |
| `ContainerOomKilled` | Docker currently reports `OOMKilled` for a Compose service | Adds service attribution when Docker retains it; the host OOM counter remains the durable fallback after an automatic restart clears the current flag. |
| `MeasuredMemoryEnvelopeExceeded` | `sybil-history` or `sybil-web` current cgroup usage above 160 MiB for 15 minutes | Both thresholds come from measured post-warm-up behavior and catch the observed allocator/cache and V8 failure modes before the much larger generic cgroup percentage is actionable. Sparse probe samples remain continuous through a ten-minute lookback. |
| `ContainerMemoryHigh` | Current cgroup usage above 85% of its configured limit for 10 minutes | Gives a bounded service time to plateau or shed load before an OOM; current, creation-lifetime peak, and limit are retained as five-minute series. |
| `ContainerMemoryCritical` | Current cgroup usage above 95% of its configured limit for 2 minutes | This is the final actionable margin before a cgroup OOM kill and is deliberately independent of point-in-time health. |
| `HostDiskSpaceLow` | Root filesystem below 15% available for 10 minutes | Both named sequencer/history Docker volumes currently allocate from this filesystem, so the warning precedes correlated redb failures. |
| `HostDiskSpaceCritical` | Root filesystem below 5% available for 2 minutes | This is an incident threshold, not permission to delete an unacknowledged source row or canonical artifact. |
| `SequencerAccountStockGrowingFast` | More than 100 committed accounts added in 15 minutes, sustained for 5 minutes | Accounts are permanent today and each account recurs in every later full-state witness. This is a traffic-anomaly warning, not a hard capacity limit. |
| `PublicAccountCapacityLow` | At most 10% of configured lifetime public account stock remains for 5 minutes | Gives the operator time to communicate exhaustion or ratify a new ceiling; ids cannot be reclaimed. |
| `PublicAccountCapacityExhausted` | No public slots remain for 2 minutes | Anonymous onboarding is deterministically returning 409 and will not self-recover. |
| `ProductHistoryOutboxStale` | Oldest unacknowledged batch older than 15 minutes for 5 minutes | Normal delivery deletes acknowledged prefixes quickly; age detects a stopped or slower-than-ingress projector without assuming batches have uniform size. |
| `ProductHistoryOutboxLarge` | Encoded source payloads above 256 MiB for 5 minutes | This is an early warning below normal host capacity. It excludes redb key/page/fragmentation overhead, which the filesystem alerts cover. |
| `WitnessPayloadLarge` | Rolling p99 canonical witness payload above 8 MiB for 5 minutes | At the 10-second deployment cadence, retaining an 8 MiB payload every block is about 68 GiB/day before database overhead. The threshold is an early operating-cost warning, well below host-memory exhaustion. |
| `DaArtifactWriteBacklog` | More than two DA writes in flight for 2 minutes | One write per block is expected. Sustained overlap means the post-commit writer is not keeping pace and can consume memory through cloned witnesses and queued write tasks. |

`sybil_da_artifact_persist_duration_seconds` is supporting context. Compare its
rolling p99 with the configured block interval; persistence consistently near
or above that interval explains a growing in-flight count. DA payload, latency,
and in-flight metrics exist only when `SYBIL_DATA_DIR` enables the artifact
path. `sybil_state_accounts_total` is emitted on every committed block.

Product-history stock metrics are emitted whenever a persistent sequencer store
exists, even if `SYBIL_HISTORY_URL` is missing. The payload-byte counter is
initialized once for an older store and then updated in the same redb
transactions that insert or acknowledge rows; routine polling does not rescan
the backlog. Use these together:

- `sybil_product_history_outbox_backlog_rows`
- `sybil_product_history_outbox_payload_bytes`
- `sybil_product_history_outbox_oldest_height`
- `sybil_product_history_outbox_newest_height`
- `sybil_product_history_outbox_oldest_age_seconds`

Payload bytes are exact encoded value bytes, not total disk allocation. The
root-filesystem ratio is the final capacity signal because redb pages,
fragmentation, canonical qMDB/redb, the history projection, Docker logs, and
other services share the host.

The API's canonical sequencer store runs with a 128 MiB redb page-cache ceiling
(`SYBIL_SEQUENCER_REDB_CACHE_BYTES=134217728`) inside its 1.25 GiB cgroup.
`sybil-history` independently uses a 64 MiB ceiling
(`SYBIL_HISTORY_REDB_CACHE_BYTES=67108864`) inside its 256 MiB cgroup. Redb's
upstream 1 GiB default is intentionally not used for either store: page-cache
warm-up otherwise looks like a gradual process leak and can consume the whole
history budget or leave the API without recovery headroom. Compose also sets
`MALLOC_ARENA_MAX=2` for `sybil-history`: blocking-query workers can otherwise
leave many glibc arenas resident after the worker threads exit, allowing
anonymous RSS to grow independently of the redb cache ceiling. Confirm both
cache values and the allocator bound in the resolved Compose configuration
before attributing a slope to application state. Treat sustained history
memory above roughly 160 MiB after cache warm-up as unexpected; separate
anonymous RSS from reclaimable file cache, compare live thread count with the
anonymous arena mappings in `/proc/1/smaps`, and inspect query concurrency
before raising either limit. The API cache ceiling is not a total-RSS ceiling:
correlate it with qMDB/account state, recent blocks, queues, and the existing
absolute and slope alerts.

The production `sybil-web` boundary is a Next standalone server inside a
512 MiB cgroup because dynamic `/m/[id]` routes and the allowlisted runtime
image optimizer still need server execution. The market index itself is a
prerendered 15 KiB shell; the browser fetches the canonical market catalog
after hydration. With that boundary, 600 index requests peaked at 87 MiB and
recovered to 72 MiB, while 1,200 dynamic-market requests peaked at 129 MiB and
settled near 118 MiB. Normal post-deploy traffic has remained around
92-101 MiB with a roughly 141 MiB creation-lifetime peak. Treat sustained use
above roughly 160 MiB after warm-up, or monotonic route-correlated growth, as
unexpected rather than as a reason to consume the 512 MiB safety margin.

For frontend growth, separate the route class before changing limits. The
former server-rendered index fetched a roughly 564 KiB catalog for every
request and serialized it into an approximately 280 KiB RSC response, retaining
memory until V8 exited 139 near its heap ceiling; Docker reported
`OOMKilled=false`. Compare lightweight routes, `/`, and `/m/[id]`, then inspect
both the Next/V8 fatal log and the retained synthetic
restart/start-time/OOM/current/peak series. A healthy container after an
automatic replacement does not prove a plateau, and a V8 heap abort is not
necessarily a memcg OOM.

The in-flight gauge increments before each DA task is spawned and decrements
after its write result is recorded. A task panic deliberately leaves the gauge
high for that process lifetime, making the lost writer visible; restarting the
process resets the metrics recorder. Treat an unexplained stuck value as a task
failure and inspect logs before restarting.

## Triage

1. Confirm the API is still producing blocks and inspect
   `sybil_persistence_failures`, `sybil_da_artifact_persist_failures_total`,
   `sybil_process_resident_memory_bytes`, root-filesystem free space, and
   container logs.
   For an OOM alert, use `journalctl -k` to distinguish a global host OOM from
   a cgroup kill; record the killed process's anonymous/file RSS and cgroup.
   `just status` reports pending/firing alerts plus each service's current
   restart count and OOM flag; query
   `sybil_synthetic_container_restart_count` and
   `sybil_synthetic_container_started_at_seconds` over the incident window;
   the latter remains discontinuous when a deliberate recreation resets the
   Docker counter.
2. For disk pressure or history backlog, compare outbox rows, payload bytes,
   oldest age/height, newest height, and `sybil_block_height`. Check
   `sybil-history` health/logs, the dedicated token, network, history-volume
   writes, and whether acknowledgements are advancing. A timeout after a
   successful remote apply is safe: redelivery is idempotent and the next
   acknowledgement removes the source row. A permanent validation conflict is
   an integrity incident; preserve both stores.
3. For RSS growth, correlate `sybil_recent_block_cache_len`,
   `sybil_product_history_outbox_backlog_rows`, outbox payload bytes, witness
   size, `sybil_state_accounts_total`, pending orders, and actor queue. A flat
   queue with growing durable state or outbox stock suggests retained data; a
   growing queue with flat state suggests backpressure. Once PSI is high, treat
   queue growth as possibly downstream of host starvation.
   The derivative alert requires the recent-block cache length to be stable
   across its complete 30-minute slope window, then requires that slope to
   persist for 30 minutes. Startup/cache growth therefore cannot enter a
   purported post-warm-up measurement. Absolute RSS alerts remain active
   throughout startup.
4. For account growth, compare `sybil_state_accounts_total` with
   `sybil_public_account_stock`, `sybil_public_account_remaining`, and
   `sybil_public_account_creation_total{result=...}`. Anonymous creation uses
   `POST /v1/onboarding/accounts`; `POST /v1/accounts` is a trusted service/dev
   bypass. Inspect route/client rate-limit rejections before changing the
   lifetime ceiling. Do not assume restart or deletion will recover ids.
5. For a large witness, compare `sybil_state_accounts_total`,
   `sybil_pending_orders`, and `sybil_quarantine_ledger_size`. These are the
   principal recurring full-state populations currently exposed as metrics.
6. For a DA backlog, compare p99 DA persistence duration with block cadence and
   check redb volume latency/free space. A rising in-flight gauge with healthy
   block production means committed blocks may temporarily lack retained DA
   artifacts.
7. Verify a recent manifest and payload through the service-gated DA endpoints
   after the backlog clears. Investigate any increment in
   `sybil_da_artifact_persist_failures_total` even if the backlog gauge recovers.

Do not delete the store merely to clear these warnings. Product-history source
rows are deleted only after the private projector has durably applied the exact
payload; block/DA retention is a different policy and cannot certify product
history. If space is critical, stop discretionary writers, preserve both
volumes, restore projector throughput, and escalate the explicit overflow
decision tracked in [GitHub #90](https://github.com/MetaB0y/sybil/issues/90).
Do not improvise a silent-drop floor. Account and live-order stock are current
state and are not repaired by history pruning.
