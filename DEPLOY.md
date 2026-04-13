# Deployment Runbook

## Architecture

```
GitHub Actions ── build Docker image ──→ GHCR (ghcr.io/metab0y/sybil-api)
                                              │
Linode (g6-standard-1, 2GB) ←── docker pull ──┘
  ├── sybil-api        (port 3000, Traefik proxy)
  ├── sybil-polymarket  (sidecar, connects to sybil-api)
  ├── VictoriaMetrics   (metrics, port 8428)
  ├── Tempo             (traces, port 4317)
  └── Grafana           (dashboards, port 3001)
```

Both `sybil-api` and `sybil-polymarket` binaries are in the same Docker image.
Kamal deploys `sybil-api` as the main service; `sybil-polymarket` runs as an accessory.

## Prerequisites

- Linode account with a running instance
- GitHub account with GHCR access
- Kamal 2 installed: `gem install kamal`
- SSH key added to the Linode

## Server

- **Provider**: Linode
- **Type**: g6-standard-1 (1 vCPU, 2GB RAM, 50GB disk)
- **OS**: Debian 13
- **IP**: set in `config/deploy.yml`

## Secrets

```bash
cp .kamal/secrets.example .kamal/secrets
```

Fill in:
```
KAMAL_REGISTRY_USERNAME=<github-username>
KAMAL_REGISTRY_PASSWORD=<ghcr-pat-with-write:packages>
SYBIL_SEED_MARKETS=
```

The server IP is hardcoded in `config/deploy.yml`. Secrets file is gitignored.

## CI (GitHub Actions)

On every push to `main`, `.github/workflows/docker.yml`:
1. Builds the Docker image (both binaries)
2. Pushes to `ghcr.io/metab0y/sybil-api:latest` and `:sha`
3. Uses GHA cache for layer caching

## Deploy

### First time

```bash
# 1. Ensure Docker image exists on GHCR (push to main or run workflow manually)
# 2. Set up the server (installs Docker, starts proxy + accessories)
kamal setup
# 3. Boot the polymarket mirror
kamal accessory boot polymarket
```

### Subsequent deploys

```bash
# After pushing to main and CI builds the image:
kamal deploy
```

### Accessory management

```bash
kamal accessory boot polymarket       # start
kamal accessory stop polymarket       # stop
kamal accessory restart polymarket    # restart
kamal accessory logs polymarket       # view logs
kamal accessory boot --all            # start all (metrics, grafana, etc.)
```

## Operations

### View logs

```bash
kamal app logs -f                 # sybil-api logs
kamal accessory logs polymarket   # polymarket mirror logs
```

### SSH into server

```bash
kamal app exec -i bash            # shell in sybil-api container
ssh root@<server-ip>              # direct SSH
```

### Check health

```bash
curl http://<server-ip>:3000/v1/health
curl http://<server-ip>:3000/        # dashboard
```

### Restart

```bash
kamal app restart                 # restart sybil-api
kamal accessory restart polymarket
```

## Services

### sybil-api

- **Port**: 3000
- **Healthcheck**: `GET /v1/health`
- **Dashboard**: `GET /`
- **Config**: env vars in `config/deploy.yml`
  - `SYBIL_DEV_MODE=true` (required for market creation)
  - `SYBIL_BLOCK_INTERVAL_MS=2000`

For the ad-hoc SSH `just deploy-api` path, `sybil-api` runs on Docker bridge networking
while the OTEL collector is exposed on the host. Set
`OTEL_EXPORTER_OTLP_ENDPOINT=http://172.17.0.1:4317` so traces reach the host-published
collector instead of container-local `localhost`.

### sybil-polymarket

- **Connects to**: `http://host.docker.internal:3000`
- **Config**: cmd args in `config/deploy.yml` accessories section
  - `--max-events 50`
  - `--mm-half-spread 0.02`
  - `--mm-budget-dollars 5000`
  - `--mm-initial-balance-dollars 1000000`
  - `--mapping-store-path /data/polymarket_mapping.json`
- **Persistent volume**: `polymarket-data:/data` (mapping store survives restarts)

### Monitoring

- **Grafana**: `http://<server-ip>:3001` (admin/admin)
- **VictoriaMetrics**: `http://<server-ip>:8428`
- **Metrics endpoint**: `http://<server-ip>:3000/metrics` (Prometheus format)

## Troubleshooting

### sybil-polymarket can't connect to sybil-api
Check that sybil-api is healthy first: `curl http://localhost:3000/v1/health` on the server.
The polymarket mirror retries until the API is up.

### No markets appearing
Check polymarket mirror logs: `kamal accessory logs polymarket`.
Common issue: Polymarket Gamma API may be rate-limiting or returning errors.

### WebSocket disconnects
The feed actor reconnects automatically with exponential backoff.
Proactive reconnect every 15 minutes to preempt Polymarket's known zombie connection bug.

### OOM on build
Don't build on the server — use GitHub Actions. The Rust compile needs ~4GB RAM.
If you must build remotely, use a temporary larger instance.
