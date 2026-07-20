---
tags: [infrastructure, operations, deployment]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-20
---

Sybil runs the same API/history images in four different postures. `local` and
`devnet` permit deliberate development tradeoffs; `prelaunch` and `prod`
fail-close if those tradeoffs leak into a locked deployment. This note is the
source of truth for which durability, cache, and prover knob belongs in each
profile and documents the startup guardrail (SYB-133). See [[REST API]] for the
endpoints these knobs feed and [[Sybil Architecture]] for the system overview.

## Deployment profiles

`SYBIL_DEPLOYMENT_PROFILE` (`local` | `devnet` | `prelaunch` | `prod`, process default
`local`) names the intended posture and drives the preflight guardrail.

- **local** — developer laptop / CI. `docker compose up` (base + override)
  starts the minimal API/history/web core, or use `cargo run`. Dev conveniences
  on, no durability expected.
- **devnet** — a shared public development network (base
  `docker-compose.yml` plus explicitly selected service profiles). Dev-tuned
  and multi-user; no production guarantees. Base Compose explicitly selects
  `devnet`, so its startup log cannot silently self-label as local.
- **prelaunch** — the operator-only play-money sandbox currently deployed
  through base Compose plus `docker-compose.prod.yml`. It keeps production
  persistence, service authentication, WebAuthn pins, and fail-closed
  guardrails while requiring a bounded fixed public account grant. The locked
  product overlay selects this profile by default.
- **prod** — real-value posture (the same product overlay with explicit
  `SYBIL_DEPLOYMENT_PROFILE=prod` and
  `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS=0`). It permits no play-money minting and
  fail-closes every dev-only deviation.

Compose service profiles are deliberately separate from the process posture
above. The unprofiled default is the core: `sybil-api`, `sybil-history`, and
`sybil-web` (plus Caddy in the locked product overlay). Optional subsystems are:

- `integrations` — native catalog/admin/MM, Polymarket mirror, and Arena runner/dashboard;
- `validity` — the durable prover daemon;
- `ops` — VictoriaMetrics, vmalert, Grafana, and node-exporter;
- `l1-indexer` — the separately credentialed L1 lifecycle sidecar.

`just docker-up-all` selects the first three on a workstation. The 2 GB
prelaunch deploy recipes select `integrations` and `ops`; both `validity` and
`l1-indexer` remain independent opt-ins. Product Compose sets
`SYBIL_RETAIN_VALIDITY_ARTIFACTS=false`, so it keeps native verification,
recovery state, replay blocks, and product history without building portable
proof jobs or DA serving payloads that have no consumer. Validity stays off
because the release does not claim ZK/TEE/L1 security and the current mock
daemon's retained job stock is not bounded on that host
([#137](https://github.com/MetaB0y/sybil/issues/137)).

`docker-compose.validity.yml` is a chain-mode overlay, not a process toggle: it
sets artifact retention back to `true` and exposes the prover scrape target.
The store binds that choice before block 1 and refuses an in-place change or an
unbound older chain. `just deploy-prover-daemon CONFIRM` therefore clears all
coupled state and starts the validity topology from fresh genesis.

## Profile matrix

Values are the effective settings after Compose overrides. "devnet" reflects
base `docker-compose.yml`; "prelaunch" reflects the default base +
`docker-compose.prod.yml` product stack. The real-value `prod` posture
uses that same overlay with the two explicit funding overrides above.

### Trust boundary

| Knob | local | devnet | prelaunch / prod | Dev-only in locked profiles? |
| --- | --- | --- | --- | --- |
| `SYBIL_DEPLOYMENT_PROFILE` | `local` | `devnet` | `prelaunch` / `prod` | — |
| `SYBIL_DEV_MODE` | `true` | `true` | `false` | **yes — blocks** |
| `SYBIL_SERVICE_TOKEN` | unset | unset | **set** (required) | **yes — blocks** |
| `SYBIL_HISTORY_URL` | compose service | compose service | `http://sybil-history:3003` | **yes — blocks** |
| `SYBIL_HISTORY_TOKEN` | Compose dev secret | Compose dev secret | **set, dedicated** | **yes — blocks** |
| `SYBIL_ARENA_READ_URL` | compose service | compose service | `http://sybil-arena:9103` | **yes — blocks** |
| `SYBIL_ARENA_READ_TOKEN` | Compose dev secret | Compose dev secret | **set, dedicated** | **yes — blocks** |
| `SYBIL_CORS_ORIGINS` | permissive (dev) | permissive (dev) | explicit allowlist | no |
| `SYBIL_HTTP_TRUSTED_PROXY_CIDRS` | empty (peer IP only) | empty (peer IP only) | exact audited Caddy-facing CIDR or empty | no |
| `SYBIL_ALLOW_DEV_KNOBS` | n/a | n/a | `false` | override only |

### Durability / persistence

| Knob | local | devnet | prelaunch / prod | Dev-only in locked profiles? |
| --- | --- | --- | --- | --- |
| `SYBIL_DATA_DIR` | `/data` in Compose; unset for direct `cargo run` | `/data` (redb) | `/data` (redb) | **yes — blocks** |
| `SYBIL_MARKET_REF_DATA_PATH` | unset (volatile) | unset (volatile) | `/data/market_ref_data.json` | no (degraded) |
| `SYBIL_ADMIN_FEED_KEY_PATH` | unset (ephemeral) | unset (ephemeral) | `/data/admin-feed.key` | **yes — blocks** |
| `SYBIL_EVENT_SNAPSHOT_DIR` | unset | `/data/event_snapshots` | `/data/event_snapshots` | no |
| `SYBIL_HISTORY_DATA_DIR` | `/history-data` in Compose | `/history-data` | `/history-data` | enforced by history process |
| `SYBIL_HISTORY_MAX_QUERY_CONCURRENCY` | `16` | `16` | `16` | no |
| `SYBIL_RETAIN_VALIDITY_ARTIFACTS` | `true` | `true` | `false` for product / `true` with validity overlay | chain identity; fresh genesis required |

### Cache / history caps

| Knob | default | devnet | prelaunch / prod | Dev-only in locked profiles? |
| --- | --- | --- | --- | --- |
| `SYBIL_RECENT_BLOCK_CACHE_CAPACITY` | `100` | `100` | `100` | no |
| `SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS` | `0` (no prune) | `0` | `60480` (7 days at 10s/block) | no |
| `SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS` | `8640` in Compose; `0` for direct runs | `8640` (1 day at 10s/block) | `60480` (7 days at 10s/block) | no |
| `SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS` / `MAX_ROWS_PER_PASS` | `60` / `1000` in Compose | `60` / `1000` | `60` / `10000` | no |
| `SYBIL_CANONICAL_ARCHIVE_MAINTENANCE_INTERVAL_BLOCKS` / `MAX_ROWS_PER_PASS` | `1000` / `10000` | same as default | `60` / `10000` | no |
| `SYBIL_MIN_RESTING_ORDER_NOTIONAL_NANOS` | `1000000` | `1000000` | `1000000` | no |
| `SYBIL_HTTP_DA_GLOBAL_RPS` / `BURST` | `20` / `40` | `20` / `40` | `20` / `40` | no |
| `SYBIL_HTTP_DA_CLIENT_RPS` / `BURST` | `10` / `20` | `10` / `20` | `10` / `20` | no |
| `SYBIL_HTTP_DA_MAX_CONCURRENCY` | `4` | `4` | `4` | no |
| `SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS` | `256` | `256` | `256` | no |
| `SYBIL_WS_CLIENT_IDLE_TIMEOUT_MS` | `90000` | `90000` | `90000` | no |
| `SYBIL_REFERENCE_PRICE_TTL_MS` | `60000` | `60000` | `60000` | no |
| `SYBIL_PUBLIC_ACCOUNT_CAPACITY` | `1000` | `1000` | `1000` (override deliberately) | no |
| `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS` | `1000000000000` ($1,000 play money) | same | `1000000000000` / `0` | **prelaunch exception; blocks prod when nonzero** |
| `SYBIL_HTTP_ONBOARDING_GLOBAL_RPS` / `BURST` | `5` / `20` | `5` / `20` | `5` / `20` | no |
| `SYBIL_HTTP_ONBOARDING_CLIENT_RPS` / `BURST` | `1` / `3` | `1` / `3` | `1` / `3` | no |

The sequencer has no general fill, account-event, equity, or chart-price history
cache. Product-history stock and historical query policy live in
`sybil-history`; the initial service retains raw batches and projections without
an age/row cap. The remaining recent-block ring and compact rolling aggregate
anchors support hot block serving and current-value calculations only.

The 256 anonymous-stream ceiling is a hard admission budget for public
WebSocket connections, not a capacity claim. Run `just ws-load` with at
least 100 subscribers before changing it or claiming fanout headroom; the
runbook requires concurrent RSS/high-water, mailbox, solve-p99, health-p95, and
block-progress evidence plus a separate fast-cadence lag/replay profile.

The reference-price TTL is an API-owned per-market safety ceiling, independent
of the mirror's 30-second per-token staleness detector. Public market reads omit
expired values, and restart begins empty until the mirror republishes; changing
this knob affects off-block display/agent inputs only, never matching or
committed state.

Public onboarding has both a flow and a stock boundary. The route-specific
token buckets reject bursts before cryptographic/actor work; a dedicated
sequencer-owned public counter enforces the lifetime stock cap across restarts
and concurrent callers. Service accounts share the account-id sequence but do
not consume this anonymous grant stock. With the Compose defaults, anonymous demo minting is bounded to 1,000
accounts × $1,000 = $1,000,000 of non-redeemable play money. Service-authenticated
account creation remains a trusted operator bypass and is therefore not an
anti-compromise control. Account ids are never reclaimed or reused. A real-value
`prod` profile sets the public grant to zero; a nonzero override blocks startup
unless the loud dev-knob escape hatch is used. Conversely,
`prelaunch` requires both nonzero account capacity and a nonzero fixed
grant, so a product deployment cannot silently present an unfunded demo-account
flow again. Real-value identity funding must arrive through the capital-backed
path, with monitoring retained for total stock and remaining public capacity.

### Prover

The explicit `validity` Compose profile runs one restart-safe `sybil-prover
daemon`, with separate redb and artifact volumes and authenticated pull/ack
against the API outbox. Its Compose configuration selects the typed mock
backend for bounded integration tests; the repository daemon default is STARK.
The scheduler, proof-job source, and HTTP server are all process-critical:
an error, panic, or clean early return from any one broadcasts shutdown to its
siblings, drains the HTTP server, and exits nonzero so Compose's bounded
`on-failure` policy can act. `/readyz` is an additional serving contract, not a
replacement for process supervision.
The `docker-compose.validity.yml` overlay also enables source proof-job/DA
retention and swaps VictoriaMetrics' empty prover discovery file for the exact
daemon target. It must be selected from block 1; use
`just deploy-prover-daemon CONFIRM`, never add the profile to a running product
chain.
It is not part of the 2 GB prelaunch host. A live soak reached 303 MiB anonymous
RSS and its 384 MiB cgroup ceiling after only 140 retained jobs, so the profile
uses bounded restart attempts and remains opt-in while #137 defines and
implements retention. Production-capable STARK mode runs from a pinned
repository checkout on measured prover hardware. Mock and STARK envelopes are
both ineligible for L1 submission; EVM/Halo2 remains disabled under GitHub #13.
See the [prover runbook](../../runbooks/prover-daemon.md).

## Startup preflight guardrail (SYB-133)

At boot, before opening the store or binding the socket,
`sybil-api` runs a preflight (`run_preflight`) that:

> `crates/sybil-api/src/preflight.rs`

1. **Logs one structured block** — the active profile plus every knob whose
   value diverges from the prod-intended baseline, tagged `DEV-ONLY` when the
   value is unsafe in a real-value posture (`deployment profile preflight` info line).
   This runs on **every** profile, so a `local` or `devnet` box still surfaces
   its deltas.
2. **Fail-closes locked starts.** `prod` rejects every dev-only knob:
   `SYBIL_DEV_MODE=true`, service/history token unset, history URL unset,
   `SYBIL_DATA_DIR` unset, `SYBIL_ADMIN_FEED_KEY_PATH` unset, or
   `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS` nonzero. `prelaunch` applies the same
   list except that its fixed play-money grant is permitted and required. The
   process exits non-zero with a message naming the offending knobs. This
   mirrors the existing fail-closed service-token posture in `service_auth`
   (`crates/sybil-api/src/app.rs`), promoted from request-time to startup.
3. **Override**: `SYBIL_ALLOW_DEV_KNOBS=1` downgrades the refusal to a loud
   `tracing::error!` and lets the process start — a fail-open escape hatch for
   deliberate one-off operations, never steady state.

`local` and `devnet` never block. `prelaunch` and `prod` fail closed.

## L1 indexer finality and cursor policy

The `sybil-l1-indexer` is an opt-in Compose profile until a vault/RPC deployment
is configured. The server image packages the binary; the profile mounts a
dedicated cursor volume and exposes independent `/metrics` and `/healthz` on
port 9102. It always requires `SYBIL_L1_CURSOR_PATH`. Cursor schema v3 binds
chain id, vault address, trust mode, sorted non-secret provider identities,
`next_from`, and the canonical hash of the last fully processed block in one
durable update. It also retains the last authenticated source-tip header so a
finalized-height regression cannot hide above a chunked scan checkpoint. An
old cursor or a changed provider set is rejected rather than
blessed from the current RPC view. A detected reorg, finality regression,
provider disagreement, invalid hash binding, or root mismatch adds a persistent
integrity latch that restarts refuse. Fatal startup or runtime failures retain
only the metrics and unhealthy health endpoints so the first scrape cannot
lose the incident. That fatal metrics-only mode still listens for Ctrl-C and
Docker SIGTERM. Ordinary polling also honors those signals, cancels the
in-flight poll, and gracefully drains the monitoring server. Every Ethereum
RPC and Sybil API request shares a configurable end-to-end deadline
(`SYBIL_L1_REQUEST_TIMEOUT_MS`, 30 seconds by default), so one silent provider
cannot leave the last readiness snapshot looking healthy forever; see the
[L1 reorg runbook](../../runbooks/l1-reorg-recovery.md).

Local Anvil explicitly uses `unsafe-single-dev` and may set both confirmation
values to zero. Public/devnet operation requires `unanimous-finalized`, at
least two comma-separated URLs, and matching unique operator-assigned provider
ids. It chooses the common finalized prefix and requires unanimous
block-hash-pinned logs and canonical state calls, under an explicit
at-least-one-honest-independent-provider assumption. Real-value operation
remains blocked on complete state recovery for already-applied bridge events
and the other incomplete production items in the L1 architecture.

The API independently requires one all-or-none bridge admission domain:
`SYBIL_BRIDGE_CHAIN_ID`, `SYBIL_BRIDGE_VAULT_ADDRESS`, and
`SYBIL_BRIDGE_TOKEN_ADDRESS`. Base Compose passes the three optional values
through to `sybil-api`; when absent, monetary bridge writes fail closed without
preventing ordinary trading or status reads. For the unsafe Sepolia mock, all
three values and the indexer's chain/vault settings must come from the same
validated deployment manifest. The relay checks that equality again against
live contract wiring before sending a transaction.

## Witness and proof-job retention policy

- Block witnesses persist to the `block_witnesses` redb table **only when a
  store is configured** (`SYBIL_DATA_DIR` set). There is **no**
  `SYBIL_PERSIST_BLOCK_WITNESSES` toggle — the ticket's hypothetical knob does
  not exist in the code.
- The convenience witness cache is **latest-only**: each block's save runs
  `table.retain(|h, _| h == current_height)`, so exactly one witness (the most
  recent block) is retained. Older full-state witnesses are dropped by design.
  > `crates/matching-sequencer/src/store.rs`
- Proving material is no longer latest-only. Before either fenced A/B qMDB slot
  can rotate, a witnessed block captures its ordered pre/post leaf proofs into
  a portable job. The job is inserted into `proof_job_outbox` in the same redb
  transaction that commits the block fence. Exact-byte acknowledgements live
  in `proof_job_acks`; a wrong digest fails closed. Unacknowledged jobs remain
  indefinitely because the sequencer is still their durable owner. After the
  standalone prover durably ingests and acknowledges the exact bytes, the
  sequencer retains a configurable source safety window and then deletes the
  matching job/ack pair atomically in bounded maintenance passes. A durable
  rotating scan cursor limits rows examined as well as rows deleted, so a long
  unacknowledged prefix neither makes a pass unbounded nor starves later
  acknowledged rows. The proof-job cadence and row budget are separate from
  canonical block/DA maintenance.
- `GET /v1/blocks/{height}` replay remains backed by the bounded canonical
  block archive. The proof outbox is exposed only through
  service-authenticated oldest-unacknowledged pull and exact-digest ack routes.
  Prover database backup and artifact publication remain necessary: once an
  acknowledged source job ages past the safety window, losing the prover store
  cannot be repaired by replaying that job from the sequencer.
- A witness imported into an empty store is an explicit recovery checkpoint,
  not a claim that the fresh node can prove the incoming historical transition.
  It has no outbox row; its first locally produced child resumes mandatory job
  capture.
- DA/custody artifacts are separate from `block_witnesses`: when a store is
  configured with `SYBIL_RETAIN_VALIDITY_ARTIFACTS=true`, each committed block schedules a best-effort write to
  `da_artifacts` containing the canonical witness payload bytes and a paired
  small `da_manifests` metadata row. The public manifest endpoint reads only
  the cached metadata; the service-gated payload endpoint reads and integrity-
  checks the large artifact. Both endpoints have dedicated rate and concurrency
  limits. These rows are retained together with the existing
  canonical archive policy (`SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS` and
  `SYBIL_CANONICAL_ARCHIVE_MAX_ROWS_PER_PASS`). With `SYBIL_DATA_DIR` unset, no DA artifacts
  are retained. With block-history pruning disabled, rows remain until the
  store is reset. DA writes happen after block commit and log on failure; they
  do not roll back block production. Product-only mode retains the latest
  recovery witness but writes neither DA rows nor portable proof-job rows.

The locked product overlay gives canonical full blocks an explicit seven-day
target. In a validity deployment the same floor also bounds paired local DA
artifacts. At the inherited 10-second interval that is 60,480 heights. Product
prices/candles are not part of this job; they live in `sybil-history`.

Base Compose keeps one day of acknowledged source jobs and examines at most
1,000 old rows every 60 blocks. The locked product overlay keeps seven days
and raises the independent proof-job pass to 10,000 rows at the same ten-minute
cadence.
Direct/in-memory development retains the conservative disabled default unless
the operator opts in.

This is a row/age policy, not a disk-byte cap. Artifact sizes vary, bounded
pruning may lag, and deleting redb rows does not promise immediate filesystem
shrinkage. The job does **not** prune the latest-only recovery header/witness,
canonical fenced state, product history, or live account/order/market state.

Seven-day local DA retention is also not an escape guarantee. Once an older DA
artifact is deleted, this store cannot serve that payload for reconstruction;
and the local best-effort artifact was never an independent availability
provider. Before a production escape design relies on an accepted root, its
required snapshot/payload must be retained and tested independently of this
devnet canonical/DA budget.

## Product-history service policy

Compose runs `sybil-history` as a separate process with its own named volume.
`sybil-api` points to it through `SYBIL_HISTORY_URL`; locked profiles require
a dedicated `SYBIL_HISTORY_TOKEN` on both processes. The service is not routed by
Caddy and only `/healthz` is unauthenticated. Base Compose also keeps internal
authentication enabled with a dev-only default secret; adjacent containers do
not receive an unauthenticated private-history interface.

The sequencer requires `SYBIL_DATA_DIR` to retain the transactional outbox.
Base Compose now mounts `sybil-data` and sets `/data`, so the current devnet
actually emits the outbox. A direct in-memory `cargo run` still trades but
cannot deliver committed history. Locked-profile preflight therefore requires both canonical persistence
and the history connection/credential. A history outage returns explicit 503s
from historical endpoints while trading continues and outbox rows accumulate.

## Arena analytics ownership

The Arena worker exclusively owns `/data/decisions.db`, its schema, and all
queries over it. It serves a small bearer-authenticated read API on private port
9103 and publishes the derived `sybil_bot_*` metrics from its existing port
9101 exporter. `sybil-api` preserves the public `/v1/bots/*` contracts by
proxying typed JSON; it does not mount `arena-data`, link SQLite, or understand
Python tables. If the optional `integrations` profile is absent, bot analytics
degrade to an explicit unavailable document while trading remains healthy.

## Native market provisioning ownership

Native markets are no longer a side effect of the Polymarket sync loop.
`sybil-native-admin` is an idempotent one-shot Compose service: after the API is
healthy it validates the checked-in catalog, converges markets and groups, and
writes `/native-data/deployment.json` bound to the current genesis. Only after
that command exits successfully does `sybil-native-mm` start its independent
static-anchor flash-liquidity actor. Polymarket owns neither the native catalog,
the deployment manifest, nor the native MM account. A fresh-genesis reset
therefore clears `native-data` alongside sequencer and integration state.
Each catalog child submits the canonical creation key `native:<market-key>`.
The sequencer returns the original market id for an exact retry, including a
retry after acknowledged-write recovery, and rejects conflicting reuse. The
admin therefore never discovers identity from titles, tags, or the off-block
reference metadata written after creation.

The long-running native MM owns its operational contract on private port 9104.
`/healthz` reports process liveness, while `/readyz` requires at least one
tracked market and a recently completed live quote cycle. `/metrics` projects
the actor's read-only progress snapshot: tracked markets, observed/completed
block heights, last accepted submission block, progress time, and submission
success/failure counters. Compose checks readiness directly and
VictoriaMetrics scrapes the same owner; `sybil-api` does not infer native MM
health from orders or fills. The process listens for Ctrl-C and Docker SIGTERM.
WebSocket connection/retry and read-only refresh work are cancellation-aware;
an already-started order submission is allowed to resolve so shutdown never
turns it into an ambiguous accepted-or-dropped write. The process gives owned
tasks 35 seconds and Compose gives the process 40 seconds before escalation.
An unexpected actor or monitoring-server exit is a nonzero process failure.

The Polymarket integration follows the same owner-health rule on private port
9105, but its readiness composes all required actors: catalog sync, provider
price feed, the shared MM, and resolution when a signer is configured. Each
actor writes progress only; none reads monitoring state or coordinates through
it. Stale windows are three actor cadences with conservative floors (including
twice the MM price-expiry window for the feed). Process liveness remains a
separate `/healthz`, while `/readyz`, `/metrics`, Compose, VictoriaMetrics, and
vmalert expose which owner stopped progressing. API-side reference-price
expiry remains an independent consumer safety boundary. Catalog sync,
resolution, feed refresh, and WebSocket reconnects check the same cancellation
token between safe side-effect boundaries. Clean or failed early return from
any required actor stops the complete integration and exits nonzero; the
internal 35-second shutdown deadline sits inside Compose's 40-second grace
period.

Mirror-side remote creation is restart-aware. Every Polymarket condition maps
to a normalized, domain-separated creation key, so a lost market-create
response returns the original Sybil market rather than allocating a duplicate.
The integration durably checkpoints each returned market, group, extension,
and completed event immediately; its mapping publication syncs the file,
renames it atomically, and syncs the parent directory. Every mirrored event
also uses a genesis-bound canonical group creation key. A lost group-create
response therefore returns the exact original group; conflicting reuse fails
instead of falling back to title or membership-overlap discovery.

Persisted MM accounts are replaced only after an authoritative API 404. A
network, authentication, decode, or 5xx failure fails startup rather than
minting another funded identity. Native and Polymarket MM creation use stable
role keys (`native-mm/v1` and `polymarket-mm/v1`) at the genesis-bound service
provisioning endpoint. If a response is lost before the local account-id
checkpoint, retry returns the already funded account; changed parameters
conflict instead of creating a second identity.

The initial history redb retains raw batches, fills, events, equity, prices,
and candles without the former 30/31-day and global-row ceilings. This removes
arbitrary product truncation but makes outbox/service volume monitoring and
independent backup mandatory. Network-lifetime preservation still requires an
off-host immutable raw-batch archive and restore drills; the same-host named
volume is not an archival SLO.

The former sequencer `MAX_RECENT_*` history knobs no longer exist. The remote
projector is the sole public source for fill, event, equity, price-history, and
candle queries.

The history service accepts authenticated MessagePack batches, limits active
blocking queries to `SYBIL_HISTORY_MAX_QUERY_CONCURRENCY`, and persists the
configured candle-resolution set. Changing that set requires an explicit
reprojection/new history volume rather than silently starting a partial series.

## Admin resolution key durability

The locked product overlay pins `SYBIL_ADMIN_FEED_KEY_PATH=/data/admin-feed.key`.
On the first boot of a new `sybil-data` volume, the API generates the P256
scalar and writes it there with mode `0600`; later process and container starts
load the same key and repair broader Unix permissions to `0600`. The admin feed
therefore keeps the same public identity across ordinary
restarts and host reboots. Removing `sybil-data` is an intentional key rotation
as well as a chain-state reset, so operators must not use the reset recipe as a
routine restart mechanism. A process-level API test covers first-boot creation,
the registered public key, and restart reuse; `just compose-smoke` covers the
effective path and volume mount.

The separate remaining deployment-profile gap is that prod runs the mock
prover (see Prover section above).

## Release identity

The locked host never selects an application image through `latest`. API,
history, native, Polymarket, and optional validity binaries share one
revision-tagged `sybil-api` artifact; Arena runner/dashboard share another, and
web has a third. Each carries `org.opencontainers.image.revision`. Compose reads
the complete set plus a digest-pinned Caddy reference from
`/opt/sybil/releases/current.env`.

Promotion refuses an existing revision tag with another image ID, records the
complete set under a release id, atomically activates it, and compares every
running container's image ID with the manifest. Scoped promotion may replace
one application artifact only after a complete immutable set exists. The
matching non-secret JSON record is committed under `deploy/releases/`, outside
the host. Rollback only reactivates a retained verified set; it cannot build.
State compatibility remains a separate operator invariant, and a
consensus-incompatible rollback requires the matching backup/genesis domain.

## WebAuthn validity pins

Locked-profile startup requires the API WebAuthn policy to equal the values compiled
into shared native/guest verification: RP ID `app.172-104-31-54.nip.io`, origin
`https://app.172-104-31-54.nip.io`, and user verification enabled. A mismatch
would let the API admit an assertion the validity guest must reject, so the
deployment preflight fails closed. Serving another hostname requires an
intentional guest rebuild/repin and fresh genesis, not only changing Compose
environment variables.
