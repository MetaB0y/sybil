# Deployment Runbook

Prelaunch is a shared 12 GB host running Docker Compose from `/opt/sybil`.
Images are built locally, transferred with `docker save | ssh docker load`, and
started with the `deploy-*` recipes in the `justfile`. Application releases use
full source-revision tags; production Compose reads one recorded image set from
`/opt/sybil/releases/current.env`.

## Host Access and Safety Boundary

The current prelaunch target is the configured SSH alias `patty`
(`friend@62.171.170.238`). The former `172.104.31.54` Linode is a historical
rollback environment, not the live target for deploys, monitoring, or repairs.

This host is shared with founder-owned services and data outside Sybil. In
particular, do not alter `unbiased.service`, `perestroika-api.service`, their
files, the host PostgreSQL installation/databases, or unrelated nginx sites.
Do not reboot the host or apply host/kernel upgrades without explicit owner
approval. Sybil operations belong under `/opt/sybil`; keep its web ingress
behind the existing nginx and loopback-bound Caddy topology described below.

## Public Surface

Only Caddy publishes host ports in the locked remote Compose stack. On the
shared prelaunch host those ports are loopback-only (`127.0.0.1:3108` and
`127.0.0.1:3143`); the host's existing nginx owns public ports 80/443,
terminates TLS, and forwards Sybil hostnames to Caddy. This preserves Caddy's
application routing and dashboard authentication without altering unrelated
nginx sites or exposing Docker services directly.

| Public port | Hostname | Service | Auth |
| --- | --- | --- | --- |
| 80, 443 via nginx | `api.sybil.exchange` | `sybil-api` trading and realtime API | Public |
| 80, 443 via nginx | `app.sybil.exchange` | Next.js web UI | Public |
| 80, 443 via nginx | `arena.62-171-170-238.nip.io` | Streamlit arena dashboard | Caddy basic auth |
| 80, 443 via nginx | `grafana.62-171-170-238.nip.io` | Grafana | Caddy basic auth, then Grafana login |
| 80, 443 via nginx | `prover.62-171-170-238.nip.io` | Prover status/API | Caddy basic auth |

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
SYBIL_WEBAUTHN_RP_ID=sybil.exchange
SYBIL_WEBAUTHN_ORIGIN=https://app.sybil.exchange
GF_SECURITY_ADMIN_PASSWORD=<strong grafana admin password>
CADDY_OPS_AUTH_USER=ops
CADDY_OPS_AUTH_HASH='<bcrypt hash from caddy hash-password>'
SYBIL_CADDY_HTTP_BIND=127.0.0.1:3108
SYBIL_CADDY_HTTPS_BIND=127.0.0.1:3143
SYBIL_API_SITE=http://api.sybil.exchange
SYBIL_APP_SITE=http://app.sybil.exchange
SYBIL_ARENA_SITE=http://arena.62-171-170-238.nip.io
SYBIL_GRAFANA_SITE=http://grafana.62-171-170-238.nip.io
SYBIL_PROVER_SITE=http://prover.62-171-170-238.nip.io
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

`SYBIL_WEBAUTHN_RP_ID` is the registrable domain (`sybil.exchange`), not the
`app.` host that serves the UI. A passkey minted under it stays valid across
subdomains, so moving the app later does not force another guest repin.
`SYBIL_WEBAUTHN_ORIGIN` is the exact browser origin including `https://`, and is
deliberately narrower than the RP: only `https://app.sybil.exchange` may
authorize actions. Both are pinned inside the guests
(`crates/sybil-verifier/src/key_op_auth.rs`) and are checked at API startup by
`preflight.rs`, so a mismatch fails the boot rather than failing silently. The
RP ID must also match `NEXT_PUBLIC_WEBAUTHN_RP_ID` baked into the web image;
changing either requires a frontend rebuild.

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

## Prelaunch Canonical and Proof-Job Retention

The locked product overlay pins the canonical full-block archive and the
acknowledged proof-job safety window to seven days. At the inherited 10-second
block interval this is 60,480 heights. In a validity deployment, paired local
DA artifact/manifest rows follow the canonical archive policy; acknowledged
proof jobs become eligible only after the standalone prover has durably
accepted their exact bytes. A pass runs every 60 blocks (ten minutes) and
deletes at most 10,000 rows. `just compose-smoke` checks these effective
merged-Compose values without starting containers. Locked-profile startup also
refuses a host override that changes either retention family from 60,480;
`LockedRetentionPolicyDrift` remains armed for the loud dev-knob escape hatch.

This is not a product-history budget, hard disk quota, or escape-availability
promise. The separate `sybil-history` store currently retains raw batches,
prices, candles, account events, fills, and equity projections without pruning.
Canonical recovery state and live account/order/market state are also not
pruned by this job. Bounded maintenance can lag, redb deletion need not
immediately shrink a file, and DA needed for an accepted-root escape must be
retained independently before a real-value escape SLO can be claimed. Monitor
the history volume and root filesystem as unbounded devnet storage until an
explicit archive and retention policy is implemented. See
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

### Dependency and workflow supply chain

Every external action in `.github/workflows/` is pinned to a reviewed full
commit SHA. The adjacent tag/ref comment is part of the contract: Dependabot
updates both the immutable pin and its readable release annotation. `just
actions-pin-check` runs in `check-fast` and rejects mutable tags, branches,
aliases, unpinned container actions, or pins without a readable comment.

`.github/dependabot.yml` opens review-only weekly updates for GitHub Actions,
the root/fuzz/OpenVM Cargo workspaces, the pnpm frontend, and both uv projects.
There is no dependency auto-merge. Changes to an OpenVM lockfile or any source
in the guest closure require the normal fingerprint, commitment, consensus, and
fresh-genesis review; a green dependency PR is not permission to bypass those
gates. Deployed image digests remain a separate release concern.

Run `just audit-dependencies` locally before merging dependency updates. It
requires cargo-audit 0.22.2 and invokes pip-audit 2.10.1 exactly; the manual
`Dependency Advisory Audit` workflow additionally pins Rust 1.97.0, uv 0.11.28,
and pnpm 11.11.0. The workflow remains manual while Actions billing is disabled;
restore its weekly schedule with the other automatic workflows after billing
returns. Dependabot refresh scheduling is independent of Actions minutes.

The repository maintainer owns first triage. A new advisory fails closed unless
the affected graph is removed or a narrow, documented, time-bounded exception
is justified in `scripts/check-dependency-advisories.sh` and tracked in GitHub.
Updater failures are diagnosed from Dependabot or workflow logs; never loosen
the audit, change a consensus pin, or merge a broad lockfile rewrite merely to
make the automation green. Existing upstream-only RustSec removals are tracked
by issue #194.

```bash
just deploy-api
just deploy-arena
just deploy-monitoring
just deploy-caddy
just deploy-all
```

`deploy-all` builds the API, arena, and web images locally, transfers all three
to the host, atomically activates their immutable references, starts the
complete Compose stack, and verifies each running image ID. Because
`NEXT_PUBLIC_*` values are baked into `sybil-web`, export any overrides before
running either `just deploy-web` or `just deploy-all`.

Every successful promotion writes
`deploy/releases/<release-id>.json`. Commit that non-secret record after the
deployment; it is the outside-host evidence for the source revision, image
references, image IDs, and running-container verification. Scoped API, Arena,
and web promotions preserve the other two recorded references and refuse to
bootstrap from mutable images.

Inspect the active set or roll back to a retained set without rebuilding:

```bash
just deploy-release-verify
just deploy-rollback <release-id> CONFIRM
```

Rollback force-recreates the complete application stack and then runs the live
gate. It is appropriate only when the selected binary set can interpret the
current state. A consensus/state-incompatible rollback requires the matching
backup and fresh-genesis procedure.

`deploy-arena` and `deploy-all` require `OPENROUTER_API_KEY` in
`/opt/sybil/arena.env`. The recipes check only for the presence of required variable
names; they do not print or pass secret values in command arguments.

### Local build fast path

Build deployable images on the native Linux/amd64 development machine, not on
the shared runtime host. The deploy recipes then stream the locally built images through
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

The script scopes discovery to containers owned by the `sybil` Compose project.
It fails if any of them use host networking, publish a non-loopback host
binding, or carry OpenRouter-style key material in Docker command arrays.
Container environment values are never inspected or printed. This ownership
scope is deliberate on `patty`: host nginx, SSH, and founder-owned listeners
are outside Sybil and must not be treated as Sybil exposures or inspected for
secrets. Public API/app/auth behavior is checked separately through the HTTPS
post-deploy gate.

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
curl https://api.sybil.exchange/v1/health
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
