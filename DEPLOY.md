# Deployment Runbook

Production is a single Linode host running Docker Compose from `/opt/sybil`.
Images are built locally, transferred with `docker save | ssh docker load`, and
started with the `deploy-*` recipes in the `justfile`.

## Public Surface

Only Caddy publishes host ports in the production compose stack.

| Public port | Hostname | Service | Auth |
| --- | --- | --- | --- |
| 80, 443 | `172-104-31-54.nip.io` | `sybil-api` trading API and web UI | Public |
| 80, 443 | `arena.172-104-31-54.nip.io` | Streamlit arena dashboard | Caddy basic auth |
| 80, 443 | `grafana.172-104-31-54.nip.io` | Grafana | Caddy basic auth, then Grafana login |
| 80, 443 | `prover.172-104-31-54.nip.io` | Prover status/API | Caddy basic auth |

These services are Docker-network-only in prod and must not publish host ports:
`sybil-api`, `sybil-prover`, `sybil-arena-dashboard`, `victoriametrics`,
`vmalert`, `grafana`, `node-exporter`, `sybil-polymarket`, and `sybil-arena`.
VictoriaMetrics and vmalert are intentionally not routed through Caddy.
Grafana anonymous access is disabled; there is no intended public read-only
dashboard.

Local development still gets localhost-only port mappings through
`docker-compose.override.yml`, which is auto-loaded by plain `docker compose up`
and is not copied to the server.

## Required Prod Secrets

Create `/opt/sybil/.env` on the deploy host before running prod compose commands:

```bash
SYBIL_SERVICE_TOKEN=<strong random bearer token for service/operator routes>
GF_SECURITY_ADMIN_PASSWORD=<strong grafana admin password>
CADDY_OPS_AUTH_USER=ops
CADDY_OPS_AUTH_HASH='<bcrypt hash from caddy hash-password>'
```

`SYBIL_SERVICE_TOKEN` is injected into `sybil-api`, `sybil-polymarket`, and
`sybil-arena`. In prod, service routes fail closed when it is missing. Optional
`SYBIL_CORS_ORIGINS` may be set to a comma-separated browser-origin allowlist;
empty/unset keeps CORS same-origin only.

Generate the Caddy hash with:

```bash
caddy hash-password --plaintext '<ops password>'
```

Keep the bcrypt hash single-quoted in `.env` so `$` characters are not treated as
Compose interpolation. Optional Telegram alerting also reads
`TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` from the same file.

Create `/opt/sybil/arena.env` for the arena container:

```bash
OPENROUTER_API_KEY=sk-or-v1-...
# Optional: focus live arena bots on broad news markets.
# Defaults are ARENA_MARKET_PROFILE=all and ARENA_MAX_MARKETS=0.
# ARENA_MARKET_PROFILE=important-news
# ARENA_MAX_MARKETS=64
```

The OpenRouter key is supplied to `sybil-arena` via `env_file: ./arena.env`.
It is not passed as a CLI argument and is not interpolated into SSH command
strings by the deploy recipes. The arena process reads `OPENROUTER_API_KEY` from
its environment and exits with a clear error if it is missing.

## Deploy Commands

```bash
just deploy-api
just deploy-arena
just deploy-monitoring
just deploy-caddy
just deploy-all
```

`deploy-arena` and `deploy-all` require `OPENROUTER_API_KEY` in
`/opt/sybil/arena.env`. The recipes check only for the presence of required variable
names; they do not print or pass secret values in command arguments.

Optional Telegram alerting:

```bash
just deploy-telegram-alerts
```

## Smoke Checks

Run this on the deploy host after changes:

```bash
cd /opt/sybil
./scripts/ops-smoke.sh
```

The script fails if it finds non-loopback TCP listeners outside the Caddy port
allowlist (`80 443` by default), or if OpenRouter-style key material appears in
host process arguments or Docker command arrays.

If host SSH is intentionally public and should be tolerated by the smoke check,
run:

```bash
OPS_SMOKE_ALLOWED_PUBLIC_PORTS="22 80 443" ./scripts/ops-smoke.sh
```

## Operations

View logs:

```bash
just deploy-logs sybil-api
just deploy-logs sybil-arena
just deploy-logs grafana
```

SSH to the host:

```bash
just deploy-shell
```

Check public API health:

```bash
curl https://172-104-31-54.nip.io/v1/health
```

Reset app state only when intentional:

```bash
just deploy-reset-state CONFIRM
```
