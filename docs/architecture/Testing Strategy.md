---
tags: [testing, infrastructure]
layer: sequencer
status: planned
last_verified: 2026-07-01
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
- **Bound resource use.** Tests should run with tiny block-history, fill-history,
  and price-history caps so they prove durable fallback paths without allocating
  production-sized windows.

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
- run with history caps of 1 or 2, overflow the RAM ring, restart, and verify
  durable fallback or explicit pruned errors
- verify quiet-market chart behavior: if we promise midpoint marks for inactive
  markets, they must survive restart too

The process harness should expose `start`, `wait_health`, `kill`, `restart`,
`get_json`, `post_json`, and `wait_for_block` helpers. Keep it boring.

### 4. Docker/Deploy Smoke

Docker tests are for packaging and service wiring only:

- compose config parses
- the default profile starts core services without the optional prover worker
- health endpoint returns OK
- Prometheus scrape target for `sybil-api` is up

Do not put deep exchange semantics here. They are slower and harder to diagnose
than the in-process and process-restart layers. The current profile/config smoke
is `just compose-smoke`; it checks the prover-worker profile boundary without
starting containers.

## Next Implementation Slice

1. Add store-backed latest/list/WS replay restart tests when the historical
   block-serving adapter is implemented.
2. Add reservation-rounding properties around `notional_nanos_ceil` and
   order-book acceptance so tiny fractional orders cannot overspend.
3. Rename and organize the current restart tests around public contracts:
   acknowledged writes, committed block history, price history/candles, and
   retention.
4. Move helpers into a separate test-support crate only if multiple crates begin
   sharing the same public fixtures.

This gives us better coverage without creating a new testing platform.
