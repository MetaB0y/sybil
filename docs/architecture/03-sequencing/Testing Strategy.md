---
tags: [testing, infrastructure]
layer: sequencer
status: current
last_verified: 2026-07-13
---

# Testing Strategy

Sybil's tests should make regressions cheap to catch without turning the test
suite into another distributed system. The default path is a small ladder of
increasing realism: pure deterministic tests, in-process API tests, process
restart tests, then a tiny Docker smoke layer.

## Principles

- **Reuse existing harnesses first.** Start in `crates/sybil-api/tests/common`
  and the sequencer's existing unit-test helpers. Introduce a shared test-support
  crate only after repeated fixtures appear in multiple crates.
- **Keep prod simple.** Do not add production-only machinery for tests. Internal
  crash points should live in unit/actor/store tests; process-level tests should
  exercise real restarts around public API acknowledgements and committed blocks.
- **Test contracts, not implementation trivia.** The important contract is what
  users and bots observe: acknowledged writes survive, committed blocks are
  durable, account state is conserved, and historical reads do not require large
  RAM caches.
- **Prefer deterministic scenarios for async systems.** Property tests are most
  useful at pure boundaries such as order generation, settlement arithmetic,
  solver output validation, lifecycle state transitions, and serialization.
  Full async API/process tests should use named scenarios with explicit seeds.
- **Bound resource use.** Deterministic tests should use tiny block-history and
  projection fixtures plus bounded response pages. Capacity tests are a
  separate, explicitly invoked layer.

## Test Ladder

### 1. Pure and Actor Tests

Use regular unit tests and focused `proptest` cases in `matching-engine`,
`matching-sequencer`, and `matching-solver`.

Good properties:

- no negative balances or positions after settlement
- fill quantity never exceeds order quantity
- clearing prices stay inside `[0, 1_000_000_000]`
- buy/sell fills respect the submitted limit side
- replaying pending/control-plane WAL rows is idempotent
- serialization round trips preserve canonical bytes and hashes
- pruning only removes rows below the configured retention floor

These tests should not start an HTTP server or Docker. The first core property
slice lives in `matching-engine`: simple binary settlement deltas, minting
balance restoration, zero-fill no-ops, and order welfare/satisfaction semantics.
The first reservation property slice lives in `matching-sequencer`: buy-order
acceptance reserves ceiled fractional notional and rejects when existing
reservations consume the balance.

### 2. In-Process API Integration

Use Axum `oneshot` tests through the existing sybil-api test helpers. This layer
is for endpoint contracts, router/auth behavior, and sequencer integration
without OS process management.

The fixture shape should converge around a small `TestWorld` style helper:

- temp `SYBIL_DATA_DIR`
- deterministic market and account creation
- funded maker/taker accounts
- helpers for YES/NO buys, sells, cancels, funding, resolution, and block
  production
- configurable tiny caps for block, fill, and price history
- JSON helpers that assert status and return typed/serde values

Do this in `crates/sybil-api/tests/common` first. Moving it to a separate crate
is a later cleanup only if multiple crates need the same public helpers.

### 3. Process Restart Tests

Use real `sybil-api` child processes and a temp data directory. This layer is
for restart semantics that in-process tests can accidentally smooth over.

Required scenarios:

- acknowledge account creation, restart before the next block, verify the
  account and history event exist
- acknowledge funding, pubkey registration, market creation, metadata update,
  cancellation, template/feed installation, and resolution before the next block,
  restart, and verify each survives exactly once
- commit a trade block, restart repeatedly, and verify balances, positions,
  fills, equity, raw price history, candles, and exact-height block reads
- restart the API and history projector independently, redeliver the outbox,
  and verify remote projection completeness plus explicit 503 behavior
- verify quiet-market chart behavior: if we promise midpoint marks for inactive
  markets, they must survive restart too

The process harness should expose `start`, `wait_health`, `kill`, `restart`,
`get_json`, `post_json`, and `wait_for_block` helpers. Keep it boring.

### 4. Docker/Deploy Smoke

Docker tests are for packaging and service wiring only:

- compose config parses
- the default profile starts the durable prover daemon in typed mock mode
- health endpoint returns OK
- Prometheus scrape target for `sybil-api` is up

Keep most exchange semantics below this layer because container failures are
slower to diagnose. Two deliberately small Compose gates cover packaging and
the highest-value deployment contract:

- `just compose-smoke` parses the production Compose topology and checks the
  prover daemon/redb/source boundary plus durable API/admin-feed-key wiring without
  starting containers.
- `just itest-compose` uses an isolated project and throwaway volumes, starts
  only `sybil-api`, runs the shared `sybil-client` `seed_book` fixture over real
  HTTP, and asserts exact statuses, fills, clearing prices, reservations, and
  marked-balance conservation. It always runs `down -v` and retains container
  logs under `target/itest-compose/` on failure. `--dry-run` on the script runs
  its assertion self-test without Docker.

The fixture is `SYB-247-v1`: BuyYes 0.60 × 1000 and BuyNo 0.50 × 2000. The
partially filled NO order pins the exact YES/NO clearing vector at 0.50/0.50;
matched volume is exactly 1000 share-units. Run id 0 is single-use on fresh
state. Persistent devnet smoke callers choose a new numeric `--run-id`, which
deterministically derives disjoint P256 seeds and replay nonces.

### 5. Explicit Load and Isolation Tests

Load tests are not part of the default CI ladder. They target an already
running release stack through public HTTP and answer resource/coupling
questions that deterministic correctness tests cannot.

`crates/sybil-loadtest` uses Goose. Its first suite takes an unloaded health
baseline, saturates owner-authenticated historical account/market reads, and
continues named sequencer-health probes during load. A run fails on request
errors, insufficient samples, an absolute health p95 ceiling, or excessive p95
growth from baseline. This catches accidental sequencer actor/database work in
history handlers as well as API-runtime and same-host resource coupling.

Run the generator off-host for capacity conclusions and preserve the Goose
report with its target/profile. The exact setup and threshold variables are in
the [historical-read isolation runbook](../../runbooks/history-read-load.md).

## Next Implementation Slice

1. Add a disposable, seeded API/history process fixture for a short automated
   load-test smoke; keep capacity thresholds outside ordinary CI.
2. Add focused properties for sell-side position reservations and cancellation /
   expiry release invariants.
3. Move helpers into a separate test-support crate only if multiple crates begin
   sharing the same public fixtures.

This gives us better coverage without creating a new testing platform.
