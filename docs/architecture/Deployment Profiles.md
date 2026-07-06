---
tags: [infrastructure, operations, deployment]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-06
---

Sybil runs the same `sybil-api` binary in three very different postures. The
public 2 GB devnet box is deliberately tuned with dev-only tradeoffs — an
in-memory store, `SYBIL_DEV_MODE=true`, reduced caches — and nothing used to
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
| `SYBIL_CORS_ORIGINS` | permissive (dev) | permissive (dev) | explicit allowlist | no |
| `SYBIL_ALLOW_DEV_KNOBS` | n/a | n/a | `false` | override only |

### Durability / persistence

| Knob | local | current devnet | prod (intended) | Dev-only in prod? |
| --- | --- | --- | --- | --- |
| `SYBIL_DATA_DIR` | unset (in-memory) | unset (in-memory) | `/data` (redb) | **yes — blocks** |
| `SYBIL_MARKET_REF_DATA_PATH` | unset (volatile) | unset (volatile) | `/data/market_ref_data.json` | no (degraded) |
| `SYBIL_ADMIN_FEED_KEY_PATH` | unset (ephemeral) | unset (ephemeral) | **should be set** | no (gap, see below) |
| `SYBIL_EVENT_SNAPSHOT_DIR` | unset | `/data/event_snapshots` | `/data/event_snapshots` | no |
| `SYBIL_ARENA_DB_PATH` | unset | `/arena-data/decisions.db` | `/arena-data/decisions.db` | no |

### Cache / history caps

| Knob | default | current devnet | prod (intended) | Dev-only in prod? |
| --- | --- | --- | --- | --- |
| `SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT` | `5000` | `5000` | `5000` | no (durable-backed; `0` disables hot cache only) |
| `SYBIL_MAX_PRICE_HISTORY_POINTS_PER_MARKET` | `2000` | `2000` | `2000` | no (durable-backed) |
| `SYBIL_MAX_EQUITY_POINTS_PER_ACCOUNT` | `0` | `0` | `0` (redb-served) | no (durable-backed) |
| `SYBIL_MAX_HISTORY_EVENTS_PER_ACCOUNT` | `0` | `0` | `0` (redb-served) | no (durable-backed) |
| `SYBIL_BLOCK_HISTORY_CAPACITY` | `100` | `100` | `100` | no |
| `SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS` | `0` (no prune) | `0` | `0` | no |
| `SYBIL_RAW_PRICE_RETENTION_BLOCKS` | `0` (no prune) | `0` | `0` | no |

> Constraint note: the `SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT` compose value is
> owned by a separate lane (SYB-193 / AR-5). This doc references it but does not
> change it.

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
   `SYBIL_DEV_MODE=true`, `SYBIL_SERVICE_TOKEN` unset, or `SYBIL_DATA_DIR`
   unset. The process exits non-zero with a
   message naming the offending knobs. This mirrors the existing fail-closed
   service-token posture in `service_auth`
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
  `da_artifacts` containing the canonical witness payload bytes and typed
  manifest served by `GET /v1/da/{height}/manifest` and
  `/v1/da/{height}/payload`. These rows are retained with the existing
  `blocks_full` history policy (`SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS` and
  `SYBIL_HISTORY_PRUNE_MAX_ROWS`). With `SYBIL_DATA_DIR` unset, no DA artifacts
  are retained. With block-history pruning disabled, rows remain until the
  store is reset. DA writes happen after block commit and log on failure; they
  do not roll back block production.

## Account fill / price history serving policy (today's reality)

All four history endpoints dispatch on store presence: with `SYBIL_DATA_DIR`
set they read the full series from redb; with no store they read the bounded
in-memory ring. The RAM cap therefore only bounds the fallback. Verified by
code trace, 2026-07-03:

| Series | Endpoint | Cap default | Durable write | `cap=0` with store |
| --- | --- | --- | --- | --- |
| Equity | `/v1/accounts/{id}/equity` | `0` | unconditional | serves full series ✅ |
| History events | `/v1/accounts/{id}/events` | `0` | unconditional | serves full series ✅ |
| Price history | `/v1/markets/{id}/prices/history` | `2000` | unconditional | serves full series ✅ |
| Fills | `/v1/accounts/{id}/fills` | `5000` | unconditional per-block delta | serves full series ✅ |

- Equity, history events, price history, and fills are safe to set to `0` **only when
  `SYBIL_DATA_DIR` is set** — the durable delta is written every block
  regardless of the RAM cap, and reads fall through to redb. This is exactly
  why prod runs `SYBIL_MAX_EQUITY_POINTS_PER_ACCOUNT=0` and
  `SYBIL_MAX_HISTORY_EVENTS_PER_ACCOUNT=0`: the full series lives in redb, so
  host memory no longer grows with account count.
- With **no store** (in-memory-only), all four serve empty/partial at `cap=0` —
  durability is the only thing that makes `cap=0` viable. The preflight blocks a
  `prod` start with no `SYBIL_DATA_DIR` for exactly this reason.

### Criterion-5 gap: fills durable at `cap=0` fixed

`SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT=0` now matches equity/history/price:
the cap bounds only the in-memory hot cache. `FillRecorder` captures an
untrimmed per-block fill delta before applying retention, and `Store::save_block`
persists that delta to `FILL_HISTORY` inside the same redb block-commit
transaction as the account-state fence flip. Store-backed reads therefore serve
the durable series even when the hot window is empty.

The preflight still logs `SYBIL_MAX_FILL_HISTORY_PER_ACCOUNT=0` as an
informational deviation from the intended hot-cache default, but it no longer
blocks a `prod` boot. `SYBIL_DATA_DIR` unset remains prod-blocking.

### Secondary gaps flagged for follow-up

- `SYBIL_ADMIN_FEED_KEY_PATH` is unset in prod, so the admin resolution key is
  regenerated ephemerally on every restart. Set it to a persistent path so
  attestation-based resolution survives restarts. Logged as an informational
  deviation, not a blocker.
- Prod runs the mock prover (see Prover section above).
