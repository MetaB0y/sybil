---
tags: [infrastructure, operations, deployment]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-13
---

Sybil runs the same API/history images in three very different postures. The
public 2 GB devnet box is deliberately tuned with dev-only tradeoffs —
`SYBIL_DEV_MODE=true`, reduced caches, same-host storage — and nothing used to
stop those tradeoffs from silently leaking into a production / devnet-v2
deploy. This note is the source of truth for which durability, cache, and
prover knob is meant to hold which value in each profile, and it documents the
startup guardrail (SYB-133) that fail-closes a `prod` boot when a dev-only knob
is wired in. See [[REST API]] for the endpoints these knobs feed and
[[Sybil Architecture]] for the system overview.

## Deployment profiles

`SYBIL_DEPLOYMENT_PROFILE` (`local` | `devnet` | `prod`, default `local`) names
the intended posture and drives the preflight guardrail. It is the only new
config concept; every other row below already existed.

- **local** — developer laptop / CI. `docker compose up` (base + override) or
  `cargo run`. Dev conveniences on, no durability expected.
- **devnet** — the current shared public box (base `docker-compose.yml` alone).
  Dev-tuned but multi-user; no production guarantees. Operators should export
  `SYBIL_DEPLOYMENT_PROFILE=devnet` on this host so its startup log self-labels.
- **prod** — production / devnet-v2 (base + `docker-compose.prod.yml`). Durable,
  locked down, fail-closed. `docker-compose.prod.yml` sets
  `SYBIL_DEPLOYMENT_PROFILE=prod`.

## Profile matrix

Values are the effective settings after Compose overrides. "current devnet"
reflects base `docker-compose.yml`; "prod" reflects base + `docker-compose.prod.yml`.

### Trust boundary

| Knob | local | current devnet | prod (intended) | Dev-only in prod? |
| --- | --- | --- | --- | --- |
| `SYBIL_DEPLOYMENT_PROFILE` | `local` | `local` (set `devnet`) | `prod` | — |
| `SYBIL_DEV_MODE` | `true` | `true` | `false` | **yes — blocks** |
| `SYBIL_SERVICE_TOKEN` | unset | unset | **set** (required) | **yes — blocks** |
| `SYBIL_HISTORY_URL` | compose service | compose service | `http://sybil-history:3003` | **yes — blocks** |
| `SYBIL_HISTORY_TOKEN` | Compose dev secret | Compose dev secret | **set, dedicated** | **yes — blocks** |
| `SYBIL_CORS_ORIGINS` | permissive (dev) | permissive (dev) | explicit allowlist | no |
| `SYBIL_ALLOW_DEV_KNOBS` | n/a | n/a | `false` | override only |

### Durability / persistence

| Knob | local | current devnet | prod (intended) | Dev-only in prod? |
| --- | --- | --- | --- | --- |
| `SYBIL_DATA_DIR` | `/data` in Compose; unset for direct `cargo run` | `/data` (redb) | `/data` (redb) | **yes — blocks** |
| `SYBIL_MARKET_REF_DATA_PATH` | unset (volatile) | unset (volatile) | `/data/market_ref_data.json` | no (degraded) |
| `SYBIL_ADMIN_FEED_KEY_PATH` | unset (ephemeral) | unset (ephemeral) | `/data/admin-feed.key` | **yes — blocks** |
| `SYBIL_EVENT_SNAPSHOT_DIR` | unset | `/data/event_snapshots` | `/data/event_snapshots` | no |
| `SYBIL_ARENA_DB_PATH` | unset | `/arena-data/decisions.db` | `/arena-data/decisions.db` | no |
| `SYBIL_HISTORY_DATA_DIR` | `/history-data` in Compose | `/history-data` | `/history-data` | enforced by history process |
| `SYBIL_HISTORY_MAX_QUERY_CONCURRENCY` | `16` | `16` | `16` | no |

### Cache / history caps

| Knob | default | current devnet | prod (intended) | Dev-only in prod? |
| --- | --- | --- | --- | --- |
| `SYBIL_MAX_RECENT_FILLS_PER_ACCOUNT` | `5000` | `5000` | `5000` | no (diagnostic cache only) |
| `SYBIL_MAX_RECENT_PRICE_POINTS_PER_MARKET` | `2000` | `2000` | `2000` | no (rolling analytics only) |
| `SYBIL_MAX_RECENT_EQUITY_POINTS_PER_ACCOUNT` | `0` | `0` | `0` | no (history served remotely) |
| `SYBIL_MAX_RECENT_ACCOUNT_EVENTS_PER_ACCOUNT` | `0` | `0` | `0` | no (history served remotely) |
| `SYBIL_RECENT_BLOCK_CACHE_CAPACITY` | `100` | `100` | `100` | no |
| `SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS` | `0` (no prune) | `0` | `60480` (7 days at 10s/block) | no |
| `SYBIL_CANONICAL_ARCHIVE_MAINTENANCE_INTERVAL_BLOCKS` / `MAX_ROWS_PER_PASS` | `1000` / `10000` | same as default | `60` / `10000` | no |
| `SYBIL_MIN_RESTING_ORDER_NOTIONAL_NANOS` | `1000000` | `1000000` | `1000000` | no |
| `SYBIL_HTTP_DA_GLOBAL_RPS` / `BURST` | `20` / `40` | `20` / `40` | `20` / `40` | no |
| `SYBIL_HTTP_DA_CLIENT_RPS` / `BURST` | `10` / `20` | `10` / `20` | `10` / `20` | no |
| `SYBIL_HTTP_DA_MAX_CONCURRENCY` | `4` | `4` | `4` | no |
| `SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS` | `256` | `256` | `256` | no |

The per-account values above bound recent in-memory diagnostic/current-value
caches only. They are neither durable history nor historical query policy.
Product-history stock lives in `sybil-history`; the initial service retains raw
batches and projections without an age/row cap. Canonical portfolio state is
unaffected.

### Prover

There is **no** `sybil-api` env knob for the prover. Which prover runs is a
Compose-topology choice: `sybil-prover` (real, `serve`), the optional
`sybil-prover-worker` (behind the `prover-worker` Compose profile), and
`sybil-prover-mock`. Base `docker-compose.yml` wires `sybil-prover-mock`, and
`docker-compose.prod.yml` does **not** remove it — so **prod currently runs the
mock prover.** The preflight guardrail cannot see this (it is not a sybil-api
env var). Tracking a real-prover cutover is out of scope here; flagged for a
follow-up ticket.

## Startup preflight guardrail (SYB-133)

At boot, before opening the store or binding the socket,
`sybil-api` runs a preflight (`run_preflight`) that:

> `crates/sybil-api/src/preflight.rs`

1. **Logs one structured block** — the active profile plus every knob whose
   value diverges from the prod-intended baseline, tagged `DEV-ONLY` when the
   value is a prod-blocking tradeoff (`deployment profile preflight` info line).
   This runs on **every** profile, so a `local` or `devnet` box still surfaces
   its deltas.
2. **Fail-closes a `prod` start** when any dev-only knob is set:
   `SYBIL_DEV_MODE=true`, service/history token unset, history URL unset,
   `SYBIL_DATA_DIR` unset, or `SYBIL_ADMIN_FEED_KEY_PATH` unset. The process
   exits non-zero with a message naming the offending knobs. This mirrors the
   existing fail-closed service-token posture in `service_auth`
   (`crates/sybil-api/src/app.rs`), promoted from request-time to startup.
3. **Override**: `SYBIL_ALLOW_DEV_KNOBS=1` downgrades the refusal to a loud
   `tracing::error!` and lets the process start — a fail-open escape hatch for
   deliberate one-off operations, never steady state.

`local` and `devnet` never block; only `prod` fail-closes.

## Witness retention policy (today's reality)

- Block witnesses persist to the `block_witnesses` redb table **only when a
  store is configured** (`SYBIL_DATA_DIR` set). There is **no**
  `SYBIL_PERSIST_BLOCK_WITNESSES` toggle — the ticket's hypothetical knob does
  not exist in the code.
- Persistence is **latest-only**: each block's save runs
  `table.retain(|h, _| h == current_height)`, so exactly one witness (the most
  recent block) is retained. Older full-state witnesses are dropped by design —
  they grow redb quickly and do not yield independently provable historical
  blocks (historical qMDB slots are not retained yet).
  > `crates/matching-sequencer/src/store.rs`
- Consequence: `GET /v1/blocks/{height}` replay works from `blocks_full`, but
  independent re-proving is only possible for the latest block. This is a known
  design limitation, not a config knob.
- DA/custody artifacts are separate from `block_witnesses`: when a store is
  configured, each committed block schedules a best-effort write to
  `da_artifacts` containing the canonical witness payload bytes and a paired
  small `da_manifests` metadata row. The public manifest endpoint reads only
  the cached metadata; the service-gated payload endpoint reads and integrity-
  checks the large artifact. Both endpoints have dedicated rate and concurrency
  limits. These rows are retained together with the existing
  canonical archive policy (`SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS` and
  `SYBIL_CANONICAL_ARCHIVE_MAX_ROWS_PER_PASS`). With `SYBIL_DATA_DIR` unset, no DA artifacts
  are retained. With block-history pruning disabled, rows remain until the
  store is reset. DA writes happen after block commit and log on failure; they
  do not roll back block production.

The production overlay gives canonical full blocks and their paired local DA
artifacts an explicit seven-day target. At the inherited 10-second interval
that is 60,480 heights. Product prices/candles are not part of this job; they
live in `sybil-history`.

Production schedules the bounded pass every 60 blocks (ten minutes) with a
10,000-row delete ceiling.

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
`sybil-api` points to it through `SYBIL_HISTORY_URL`; production requires a
dedicated `SYBIL_HISTORY_TOKEN` on both processes. The service is not routed by
Caddy and only `/healthz` is unauthenticated. Base Compose also keeps internal
authentication enabled with a dev-only default secret; adjacent containers do
not receive an unauthenticated private-history interface.

The sequencer requires `SYBIL_DATA_DIR` to retain the transactional outbox.
Base Compose now mounts `sybil-data` and sets `/data`, so the current devnet
actually emits the outbox. A direct in-memory `cargo run` still trades but
cannot deliver committed history. Production preflight therefore requires both canonical persistence
and the history connection/credential. A history outage returns explicit 503s
from historical endpoints while trading continues and outbox rows accumulate.

The initial history redb retains raw batches, fills, events, equity, prices,
and candles without the former 30/31-day and global-row ceilings. This removes
arbitrary product truncation but makes outbox/service volume monitoring and
independent backup mandatory. Network-lifetime preservation still requires an
off-host immutable raw-batch archive and restore drills; the same-host named
volume is not an archival SLO.

The sequencer's `MAX_*_HISTORY_*` values now bound only optional hot analytics
caches. They do not select the public history source, which is always the
remote projector after this cutover.

The history service accepts authenticated MessagePack batches, limits active
blocking queries to `SYBIL_HISTORY_MAX_QUERY_CONCURRENCY`, and persists the
configured candle-resolution set. Changing that set requires an explicit
reprojection/new history volume rather than silently starting a partial series.

## Admin resolution key durability

The production overlay pins `SYBIL_ADMIN_FEED_KEY_PATH=/data/admin-feed.key`.
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
