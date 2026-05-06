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
- Tempo direct, when running: `http://172.104.31.54:3200`

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
- `sybil-arena` - live Python LLM/noise traders
- `sybil-arena-dashboard` - Streamlit dashboard on port `8501`
- `caddy` - HTTPS for app and arena dashboard
- `victoriametrics`, `vmalert`, `grafana`, `tempo` - observability stack

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
and `live.runner` defaults live LLM/noise orders to `IOC`; after deploying that
version, a restart clears the current in-memory book and prevents recurrence.

## Known Observability Caveat

Tempo can be killed by OOM on the 2 GB Linode. When `tempo` is exited, Grafana
metrics still work through VictoriaMetrics, but traces do not. Confirm with:

```bash
ssh root@172.104.31.54 'cd /opt/sybil && docker compose -f docker-compose.yml -f docker-compose.prod.yml ps tempo grafana victoriametrics'
```

Do not treat a missing Tempo datasource as evidence that block production or
trading is down.
