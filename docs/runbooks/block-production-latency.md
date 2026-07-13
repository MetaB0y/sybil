---
tags: [runbook, sequencing, latency]
status: current
last_verified: 2026-07-11
---

# Block-production latency

> **Executive summary:** page immediately if blocks stop. If p99 pipeline time
> rises while blocks still advance, first distinguish solver load from storage
> and host saturation; shed nonessential mirror/arena load before restarting,
> and preserve diagnostics before touching persistent state.

**Alerts:** `BlockProductionP99LatencyHigh`, `BlockProductionP99LatencyCritical`,
`BlockProductionStalled`, `SolveTimeHigh`
**Rules:** `deploy/vmalert/block-production.yml`, `deploy/vmalert/rules.yml`

---

## The SLA

Block production is single-threaded: the sequencer runs the matching solver once
per block and must finish inside the block interval (`SYBIL_BLOCK_INTERVAL_MS` —
default 500ms; the prod/dev compose set 10000ms) or blocks slip.

- **SLA:** p99 solve latency `< 100ms`, sustained (5m window).
- **Warning** — `BlockProductionP99LatencyHigh`: p99 > 100ms for 5m. The tail is
  eating into the block budget; still producing, but headroom is shrinking.
- **Critical** — `BlockProductionP99LatencyCritical`: p99 > 250ms for 5m. Solve
  has breached the critical operating threshold; cadence and the redb commit
  backlog may be at risk on faster deployment profiles.
- **Critical / top signal** — `BlockProductionStalled`: no blocks produced for 2m
  while the API is up. Production has fully stopped — page immediately.
- `SolveTimeHigh` (legacy, in `deploy/vmalert/rules.yml`) pages on the *average*
  solve time crossing 100ms. The p99 alerts above are the tail-aware SLA; a p99
  alert without a matching average alert means a small fraction of heavy blocks.

### Metric shape (important)

`sybil_solve_time_seconds` is recorded in
`crates/matching-sequencer/src/actor/production.rs`
as a metrics-rs `histogram!`. The Prometheus exporter
(`metrics-exporter-prometheus`) is installed with no custom buckets, so it renders
**summaries**: p99 is published directly as
`sybil_solve_time_seconds{quantile="0.99"}` (a rolling 60s window), with `_sum`
and `_count` for the average. **There is no `_bucket` series** — use the
`quantile` label, not `histogram_quantile()`.

---

## First diagnostics

1. **Confirm scope in Grafana / VictoriaMetrics.** In production use the
   authenticated Grafana vhost or query from inside the host/Docker network;
   local Compose exposes Grafana on `127.0.0.1:3001`, VictoriaMetrics on
   `127.0.0.1:8428`, and vmalert on `127.0.0.1:8880`:
   - `sequencer:solve_time_seconds:p99` — how far over SLA, and trending?
   - `sequencer:solve_time_seconds:avg5m` vs p99 — is the whole distribution slow
     (mean high) or just the tail (mean fine, p99 high)?
   - `sequencer:blocks_produced:rate2m` — are blocks still being produced?
   - `sequencer:orders_per_block:p99` — is this a genuine load spike?
2. **Check the sequencer is up and producing:**
   - `GET /v1/health` — sequencer liveness.
   - `GET /v1/state-root` — advancing height ⇒ blocks are committing.
   - `up{job="sybil-api"}` in VM — scrape health.
3. **Correlate with saturation alerts** that share a root cause:
   `ActorMailboxQueueHigh` / `ActorMailboxQueueCritical` (`sybil_actor_queue_depth`,
   sequencer backlog), `SybilApiMemoryHigh`, `HostCpuHigh`, `HostLoadVeryHigh`,
   `HostMemoryLow`, `HostSwapHigh`.
4. **Logs:** `docker compose logs sybil-api` — look for retained-cash iteration
   caps or solver failures, redb commit warnings, and per-block timing.

---

## Likely causes

- **Solver degradation.** The matching solver (`crates/matching-solver/`) can get
  slow or reach its certified-gap iteration cap on pathological instances
  (dense books, many groups). Production does not switch algorithms silently.
  If `avg5m` and p99 rise *together*
  with order volume (`sequencer:orders_per_block:p99`), this is load- or
  conformance-driven. Cross-check termination diagnostics; a single degenerate
  block can dominate p99 while the mean stays low.
- **redb / persistence stalls (SYB-169).** Block commit runs storage work on
  `spawn_blocking`; if that pool backs up (slow disk, fsync contention, large
  analytics deltas) the per-block wall-time — reflected in `sybil_solve_time_seconds`
  — climbs even when the solver itself is fast. Signs: `ActorMailboxQueueHigh`,
  growing `sequencer` queue depth, outbox backlog, and high host I/O.
- **Disk full / near-full.** redb writes stall or fail as the volume fills.
  Check `df -h` on the host and the `sybil-data` / `arena-data` volumes. This has
  taken down block production before; a full disk also blocks logging and metrics.
- **Host saturation.** CPU/swap pressure on the single 2GB host inflates every
  block. If `HostLoadVeryHigh` / `HostSwapHigh` are also firing, treat the host
  as the root cause (the known "TCP accepts but nothing responds" zombie mode).

---

## Mitigation

- **Sustained overload, not a bug:** reduce inbound load — lower the Polymarket
  mirror `--mm-max-orders-per-block` / `--max-events`, or pause noisy arena
  traders — until p99 is back under SLA.
- **redb / disk:** free disk, or restart `sybil-api` to drain the `spawn_blocking`
  backlog once space is recovered. Confirm the qMDB root repairs cleanly on
  restart (`StoreQmdbRootMismatch` / `StoreQmdbRepairFailed` must not follow).
- **Dev only:** `POST /v1/simulation/pause` and `/v1/simulation/resume` gate block
  production (dev-mode only, 403 in prod) — useful to stabilise while inspecting.
- **Host saturation:** shed load; if the host is in the zombie state, reboot the
  deployment host; use the external checks in
  [`synthetic-monitoring.md`](synthetic-monitoring.md) so host failure is not
  mistaken for a healthy quiet system.

---

## Escalation

1. `BlockProductionStalled` or `BlockProductionP99LatencyCritical` firing and not
   self-clearing within ~10m after load shedding → page the on-call sequencer
   owner.
2. If accompanied by `StoreQmdbRootMismatch` / `StoreQmdbRepairFailed`, treat
   persistence as unsafe: do **not** wipe the data volume; preserve it for repair
   and escalate before restarting.
3. Capture `/metrics`, recent `sybil-api` logs, and `df -h` before any restart so
   the root cause survives the mitigation.

---

## Validating the alerts

`deploy/vmalert/tests/block-production_test.yml` is a `promtool test rules` suite
proving the SLA warning fires on a synthetic 150ms-p99 overload, the critical tier
adds at 400ms, both stay silent at 50ms, and `BlockProductionStalled` fires on a
frozen block counter. Run:

```
promtool test rules deploy/vmalert/tests/block-production_test.yml
```

To route firing alerts to Telegram in the dev/observability profile, add the
`docker-compose.telegram.yml` overlay (env: `TELEGRAM_BOT_TOKEN`,
`TELEGRAM_CHAT_ID`); a generic Alertmanager-v2 webhook / PagerDuty-lite receiver
can be slotted via the commented `ALERT_WEBHOOK_URL` `-notifier.url` in that file.
