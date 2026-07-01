# Live Deployment Runbook

## Hosts and URLs

Production demo host:

- SSH: `ssh root@172.104.31.54`
- App over HTTPS: `https://172-104-31-54.nip.io`
- Arena dashboard: `https://arena.172-104-31-54.nip.io`
- Raw API: `http://172.104.31.54:3000`
- Streamlit direct: `http://172.104.31.54:8501`
- Grafana direct: `http://172.104.31.54:3001`
- VictoriaMetrics direct: `http://172.104.31.54:8428`
- vmalert direct: `http://172.104.31.54:8880`

Grafana is not currently routed through Caddy. It is exposed directly on port
`3001`; anonymous Viewer access is enabled and the provisioned admin password
is `admin`.

## Server Layout

Runtime files live under `/opt/sybil`:

```bash
ssh root@172.104.31.54
cd /opt/sybil
docker compose -f docker-compose.yml -f docker-compose.prod.yml ps -a
```

The production compose stack is the checked-in `docker-compose.yml` plus
`docker-compose.prod.yml`. The local `docker-compose.override.yml` is not
copied to the server.

Main services:

- `sybil-api` - Rust API/sequencer on port `3000`
- `sybil-polymarket` - Polymarket mirror and flash MM
- `sybil-prover` - proof artifact status API and Prometheus metrics on port `3002`
- `sybil-prover-worker` - filesystem proof-job worker
- `sybil-arena` - live Python LLM/noise traders
- `sybil-arena-dashboard` - Streamlit dashboard on port `8501`
- `caddy` - HTTPS for app and arena dashboard
- `node-exporter`, `victoriametrics`, `vmalert`, `grafana` - observability stack

## Local Image Builds

`just deploy-api` builds a `linux/amd64` image locally, transfers it with
`docker save | ssh docker load`, and restarts the production containers. The
Linode should not run the Rust release build directly.

The Dockerfile defaults to server-safe Rust settings:

- `CARGO_BUILD_JOBS=1`
- `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`

Local compose builds override those defaults through `docker-compose.override.yml`
so emergency deploys can use more of the developer machine:

```bash
SYBIL_DOCKER_BUILD_JOBS=4 \
SYBIL_DOCKER_RELEASE_CODEGEN_UNITS=16 \
just deploy-api
```

Lower `SYBIL_DOCKER_BUILD_JOBS` if the local Docker VM starts swapping. These
knobs are only for local image construction; the server receives the finished
image and the override file is not copied to `/opt/sybil`.

## Persistence

The public devnet runs `sybil-api` with `SYBIL_DATA_DIR=/data`, backed by the
named Docker volume `sybil-data`. Sequencer state is persisted at block
boundaries through the redb/qMDB store described in
`docs/architecture/Persistence.md`.

Preserved across API/container restarts:

- accounts, balances, positions, and account event digests
- markets, market metadata, statuses, and groups
- block headers and latest block witness
- resting orders and recovery logs for acknowledged-but-not-yet-committed orders
- clearing prices, market volumes, and fill history rows

Still treated as bounded live serving caches:

- recent block ring buffer
- price-history chart points
- in-memory per-account fill-history window

The in-memory cache limits are configured with:

- `SYBIL_BLOCK_HISTORY_CAPACITY`
- `SYBIL_MAX_PRICE_HISTORY_POINTS_PER_MARKET`
- `SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT`

To intentionally reset the devnet, stop the API and remove or replace the
`sybil-data` volume. Do not clear this volume as a routine restart step.

## Read-Only Checks

Use these before restarting or redeploying anything:

```bash
just status
just arena-status 24
just deploy-logs sybil-api
just deploy-logs sybil-arena
just deploy-logs sybil-polymarket
```

Direct host checks:

```bash
ssh root@172.104.31.54 'curl -sS http://localhost:3000/v1/health'
ssh root@172.104.31.54 'curl -sS http://localhost:3000/v1/blocks/latest'
ssh root@172.104.31.54 'curl -sS http://localhost:3000/metrics | grep -E "^(sybil_block_height|sybil_blocks_produced|sybil_pending_orders|sybil_pending_bundles|sybil_fills_per_block|sybil_order_submissions_total|sybil_volume_nanos|sybil_welfare_nanos)"'
ssh root@172.104.31.54 'curl -sS http://localhost:3002/metrics | grep -E "^(sybil_prover_artifact_store_ready|sybil_prover_latest_prepared_height|sybil_prover_jobs_queued|sybil_prover_latest_artifact_age_seconds)"'
```

VictoriaMetrics spot checks:

```bash
ssh root@172.104.31.54 'python3 - <<'"'"'PY'"'"'
import json, urllib.parse, urllib.request
for query in [
    "increase(sybil_fills_per_block_sum[5m])",
    "increase(sybil_order_submissions_total{result=\"accepted\"}[5m])",
    "increase(sybil_order_submissions_total{result=\"rejected\"}[5m])",
    "increase(sybil_blocks_produced[5m])",
]:
    url = "http://localhost:8428/api/v1/query?" + urllib.parse.urlencode({"query": query})
    data = json.load(urllib.request.urlopen(url))
    values = [r["value"][1] for r in data.get("data", {}).get("result", [])]
    print(query, values)
PY'
```

## Alerting

Grafana dashboard:

- `http://172.104.31.54:3001/d/sybil-overview/sybil?orgId=1`
- Anonymous Viewer access is enabled.
- Admin login is `admin` / `admin`.

vmalert evaluates rules from `deploy/vmalert/rules.yml` every 30 seconds. The
current rule set covers:

- API scrape target down for 3m
- block production stalled
- solver latency high
- actor mailbox backlog
- API process memory
- host memory, swap, CPU, and load
- high order rejection rate
- live submissions with no fills
- accepted orders with no fills
- large/stale pending order books
- prover scrape target down
- prover artifact store unreadable
- prover lagging sequencer blocks
- stale prover artifacts while blocks are producing
- prover proof-job queue backlog

Alert state is available at:

- vmalert UI: `http://172.104.31.54:8880`
- VictoriaMetrics `ALERTS` series via Grafana Explore

### Telegram Notifications

Telegram alert delivery uses the optional `docker-compose.telegram.yml`
overlay. It runs a small `telegram-alerts` bridge that accepts vmalert's
Alertmanager-compatible `POST /api/v2/alerts` payloads and forwards them to
Telegram.

Create a Telegram bot with BotFather, add it to the target chat, and get the
chat id. Then store secrets on the server:

```bash
ssh root@172.104.31.54
cd /opt/sybil
umask 077
cat >> .env <<'EOF'
TELEGRAM_BOT_TOKEN=123456:replace-me
TELEGRAM_CHAT_ID=-1001234567890
EOF
```

Enable Telegram alert delivery:

```bash
just deploy-telegram-alerts
```

After enabling, vmalert sends notifications to `telegram-alerts` instead of
`-notifier.blackhole`. Test the bridge from the server:

```bash
ssh root@172.104.31.54 'cd /opt/sybil && docker compose -f docker-compose.yml -f docker-compose.prod.yml -f docker-compose.telegram.yml exec -T telegram-alerts python - <<'"'"'PY'"'"'
import json, urllib.request
payload = [{
    "labels": {"alertname": "TelegramTest", "severity": "info", "component": "ops"},
    "annotations": {"summary": "Sybil Telegram alert test"},
    "status": "firing",
}]
req = urllib.request.Request(
    "http://localhost:8080/api/v2/alerts",
    data=json.dumps(payload).encode(),
    headers={"Content-Type": "application/json"},
    method="POST",
)
print(urllib.request.urlopen(req).read().decode())
PY'
```

## Prover Devnet Path

The deployed `sybil-prover` service exposes `/healthz`, `/proofs/{height}`,
and `/metrics`. The worker watches `/jobs/*.msgpack` and writes durable
per-block artifacts under `/artifacts`. Production proof-job export is still
being wired, so an empty prover artifact store is normal until jobs are fed
into that inbox.

For local Anvil bridge plumbing, use the explicit unsafe verifier smoke:

```bash
anvil
just contracts-anvil-unsafe-smoke
```

This deploys `MockUSDC`, `UnsafeAcceptAllVerifierAdapter`,
`SybilSettlement`, and `SybilVault`, then exercises deposit, state-root
submission, withdrawal request, and withdrawal finalization. It is separate
from production deployment and deliberately uses an accept-all verifier behind
the same `IOpenVmVerifierAdapter` boundary.

## Blocks But No Trading

Symptoms:

- `/v1/health` height increases
- latest blocks have `fill_count: 0`, `total_volume_nanos: 0`
- `sybil_fresh_orders_per_block_*` and accepted submissions are non-zero
- arena logs contain repeated `InsufficientBalance` or `InsufficientPosition`

First check pending orders:

```bash
ssh root@172.104.31.54 'python3 - <<'"'"'PY'"'"'
import collections, json, urllib.request
orders = json.load(urllib.request.urlopen("http://localhost:3000/v1/orders/pending"))
print("pending_total", len(orders))
print("by_account", collections.Counter(o["account_id"] for o in orders).most_common(20))
print("top_markets", collections.Counter(o["market_id"] for o in orders).most_common(20))
if orders:
    print("created_min_max", min(o["created_at_block"] for o in orders), max(o["created_at_block"] for o in orders))
    print("expiry_min_max", min(o["expires_at_block"] for o in orders), max(o["expires_at_block"] for o in orders))
    print("sample", orders[0])
PY'
```

If `expires_at_block` is roughly `created_at_block + 63072000`, the orders are
default GTC orders. For live arena bots that rebalance every few blocks, GTC is
usually wrong: stale resting orders reserve cash and positions, so later
rebalance orders get rejected even though the account still appears funded.

The intended live-arena mode is IOC. The Python client supports `time_in_force`
and `live.runner` defaults live LLM/noise orders to `IOC`. With persistent mode
enabled, a normal restart does not clear the book; use the pending-order checks
above and only reset `sybil-data` when intentionally starting a fresh devnet.

## Known Observability Caveat

The 2 GB Linode does not run Tempo by default. Metrics and alerts are the
operational source of truth; tracing can be enabled later by running an OTLP
collector/Tempo and setting `OTEL_EXPORTER_OTLP_ENDPOINT` for `sybil-api`.

The API runs with a Docker memory cap. If it leaks or retains too much derived
state, the desired failure mode is a `sybil-api` container restart plus alerts,
not a host-level swap spiral. The live stack also exports:

- `sybil_process_resident_memory_bytes` from `sybil-api`
- host memory, swap, CPU, and load from `node-exporter`

Host pressure can still cause brief scrape misses. Confirm the current state
with:

```bash
ssh root@172.104.31.54 'free -h; uptime'
ssh root@172.104.31.54 'curl -sS http://localhost:8428/api/v1/query?query=up%7Bjob%3D%22sybil-api%22%7D'
ssh root@172.104.31.54 'curl -sS http://localhost:8428/api/v1/query?query=sybil_process_resident_memory_bytes'
```

Do not treat a brief `SequencerDown` firing/resolved pair as evidence that
block production or trading is down unless the API health, block-production,
or no-fill alerts agree.
