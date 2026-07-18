# Deployment Runbook

Prelaunch is a single Linode host running Docker Compose from `/opt/sybil`.
Images are built locally, transferred with `docker save | ssh docker load`, and
started with the `deploy-*` recipes in the `justfile`.

## Public Surface

Only Caddy publishes host ports in the locked remote Compose stack.

| Public port | Hostname | Service | Auth |
| --- | --- | --- | --- |
| 80, 443 | `172-104-31-54.nip.io` | `sybil-api` trading and realtime API | Public |
| 80, 443 | `app.172-104-31-54.nip.io` | Next.js web UI | Public |
| 80, 443 | `arena.172-104-31-54.nip.io` | Streamlit arena dashboard | Caddy basic auth |
| 80, 443 | `grafana.172-104-31-54.nip.io` | Grafana | Caddy basic auth, then Grafana login |
| 80, 443 | `prover.172-104-31-54.nip.io` | Prover status/API | Caddy basic auth |

These services are Docker-network-only in prelaunch and must not publish host ports:
`sybil-api`, `sybil-prover`, `sybil-arena-dashboard`, `victoriametrics`,
`vmalert`, `grafana`, `node-exporter`, `sybil-polymarket`, and `sybil-arena`.
VictoriaMetrics and vmalert are intentionally not routed through Caddy.
Grafana anonymous access is disabled; there is no intended public read-only
dashboard.

Local development still gets localhost-only port mappings through
`docker-compose.override.yml`, which is auto-loaded by plain `docker compose up`
and is not copied to the server.

## Required Prelaunch Secrets

Create `/opt/sybil/.env` on the deploy host before running remote Compose commands:

```bash
SYBIL_SERVICE_TOKEN=<strong random bearer token for service/operator routes>
SYBIL_WEBAUTHN_RP_ID=app.172-104-31-54.nip.io
SYBIL_WEBAUTHN_ORIGIN=https://app.172-104-31-54.nip.io
GF_SECURITY_ADMIN_PASSWORD=<strong grafana admin password>
CADDY_OPS_AUTH_USER=ops
CADDY_OPS_AUTH_HASH='<bcrypt hash from caddy hash-password>'
```

`SYBIL_SERVICE_TOKEN` is injected into `sybil-api`, `sybil-polymarket`, and
`sybil-arena`. In prelaunch, service routes fail closed when it is missing. Optional
`SYBIL_CORS_ORIGINS` may be set to a comma-separated browser-origin allowlist;
empty/unset keeps CORS same-origin only.

`SYBIL_HTTP_TRUSTED_PROXY_CIDRS` is also optional and empty by default. Empty
means the API ignores `X-Forwarded-For`/`X-Real-IP` and conservatively shares
each per-client HTTP bucket across Caddy. If the Caddy-facing Docker network is
pinned, set this to that exact CIDR so the API can recover individual client
addresses. Do not trust a broad private range: every address in this list is
allowed to influence rate-limit identity, and Caddy must sanitize or append
`X-Forwarded-For`.

The admin resolution key is not supplied through `.env`. The locked product
overlay pins `SYBIL_ADMIN_FEED_KEY_PATH=/data/admin-feed.key`; `sybil-api`
creates it on the first boot of the persistent `sybil-data` volume and reuses
it on subsequent boots. A normal container restart therefore does not rotate
the admin feed. `just deploy-reset-state CONFIRM` deletes `sybil-data`, so it
also intentionally discards that key and creates a new chain/admin identity.

`SYBIL_WEBAUTHN_RP_ID` is the web-app hostname only; `SYBIL_WEBAUTHN_ORIGIN` is
the exact browser origin including `https://`. Both must match the app hostname
baked into the web image via `NEXT_PUBLIC_WEBAUTHN_RP_ID`; changing them
requires a frontend rebuild. The API hostname is not the relying party when the
browser ceremony runs on `app.172-104-31-54.nip.io`.

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

## Deployment Profile Guardrail

`sybil-api` reads `SYBIL_DEPLOYMENT_PROFILE` (`local` | `devnet` |
`prelaunch` | `prod`, default `local`). `docker-compose.prod.yml` defaults to
`prelaunch`, which arms the locked startup preflight while requiring the fixed
play-money onboarding grant. The server refuses to boot if another dev-only
knob is wired in (`SYBIL_DEV_MODE=true`, unset `SYBIL_SERVICE_TOKEN`, unset
`SYBIL_DATA_DIR`, or unset `SYBIL_ADMIN_FEED_KEY_PATH`). Every profile logs a
`deployment profile preflight` block naming knobs that diverge from the
real-value baseline.

Override deliberately (never steady state) with `SYBIL_ALLOW_DEV_KNOBS=1`.
Real-value deployment explicitly selects `SYBIL_DEPLOYMENT_PROFILE=prod` and
sets `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS=0`. Full knob matrix and history-serving policy:
`docs/architecture/Deployment Profiles.md`.

## Prelaunch History Retention

The locked product overlay pins the history families already supported by bounded
store pruning to seven days. At the inherited 10-second block interval this is
60,480 full blocks plus paired DA artifact/manifest rows, 60,480 raw-price
heights per market, and 604,800 seconds of each 1-minute, 5-minute, and 1-hour
candle series. A pass runs every 60 blocks (ten minutes) and deletes at most
10,000 rows. `just compose-smoke` checks these effective merged-Compose values
without starting containers.

This is a prelaunch product-history budget, not a hard disk quota or an escape
availability promise. Account events, fills, equity rows, canonical recovery
state, and live account/order/market state are not pruned by this job. Bounded
maintenance can lag, redb deletion need not immediately shrink the file, and
DA needed for an accepted-root escape must be retained independently before a
real-value escape SLO can be claimed. See
`docs/architecture/03-sequencing/Historical Data Serving.md` and the open
finding 5 in `design/dos-audit-2026-07-11.md`.

## Deploy Commands

### Release gate matrix

The release checks are cost-tiered so browser/container journeys do not inflate
the normal pull-request path:

| Gate | Workflow / command | Trigger and budget | Promotion effect |
| --- | --- | --- | --- |
| L0 property + L1 matching/fills + L2 config contract | `CI` (`check`, `smoke`, `compose-profile-smoke`) | Pull request once Actions billing is restored; no live host | Required merge checks once branch protection is enabled |
| Web unit/type/lint/build | `Frontend CI` (`build`) | Frontend pull request once Actions billing is restored | Required merge check once branch protection is enabled |
| L3 deterministic Compose money path | `Compose Integration` / `just itest-compose` | Manual pre-deploy now; nightly after billing; 45-minute hard timeout | Must be green before an operator deploys |
| L4 passkey browser journey | `Frontend CI` (`e2e`) / `pnpm e2e` | Manual pre-deploy now; future mainline/nightly, never pull requests; 20-minute hard timeout | Must be green before an operator deploys |
| L5 live-stack smoke | `just deploy-verify` | Automatic final dependency of `deploy-api`, `deploy-web`, `deploy-arena`, and `deploy-all` | Non-zero exits fail the deploy; promotion is not reported successful |

GitHub Actions is intentionally manual-dispatch-only while SYB-251's billing
limit remains in effect. Do not interpret that temporary trigger policy as a
green merge gate. After billing is restored, restore the checked-in PR/mainline
and nightly triggers, run each lane successfully to establish actual duration,
then configure the named fast checks as required in branch protection. The L4
job is explicitly guarded from pull-request events so restoring frontend PR
triggers cannot accidentally put the browser journey on the fast path.

```bash
just deploy-api
just deploy-arena
just deploy-monitoring
just deploy-caddy
just deploy-all
```

`deploy-all` builds the API, arena, and web images locally, transfers all three
to the host, and starts the complete Compose stack. Because `NEXT_PUBLIC_*`
values are baked into `sybil-web`, export any overrides before running either
`just deploy-web` or `just deploy-all`.

`deploy-arena` and `deploy-all` require `OPENROUTER_API_KEY` in
`/opt/sybil/arena.env`. The recipes check only for the presence of required variable
names; they do not print or pass secret values in command arguments.

### Local build fast path

Build deployable images on the native Linux/amd64 development machine, never on
the 2 GB Linode. The deploy recipes then stream the locally built images through
`docker save | ssh docker load`; the server only loads and starts them.

Measured on the development machine on 2026-07-10, a full-stack build with warm
cargo-chef/BuildKit caches took about one minute. A workspace-membership change
that invalidated the cold Rust dependency recipe took about nine minutes. The
former 80+ minute path was an Apple-Silicon-to-amd64 emulated build and is not a
supported deploy path. The cargo-chef recipe is manifest-driven, so adding Cargo
examples, benches, or binaries does not require maintaining dummy source files.

For routine work, batch several changes before deploying and use the narrowest
recipe that covers them (`deploy-web`, `deploy-arena`, or `deploy-api`). Use
`deploy-all` only when the complete stack changed. Preserve BuildKit's local
cache between deploys; a cold recipe rebuild is expected after dependency or
workspace-manifest changes.

The arena image also carries its offline quality-report tools. After resolved
markets exist, preview and persist their conflict-checked outcome labels, then
print the live calibration report from the shared arena volume:

```bash
just arena-outcomes-dry-run
just arena-record-outcomes
just arena-calibration
```

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

Store backup/restore and the five-minute synthetic monitor have dedicated
operator runbooks:

- `docs/runbooks/store-backup-restore.md`
- `docs/runbooks/synthetic-monitoring.md`

Reset app state only when intentional:

```bash
just deploy-reset-state CONFIRM
```

For a validity-breaking devnet redeploy (fresh genesis and verifier-adapter
repin), follow `docs/runbooks/fresh-genesis-redeploy.md`.
