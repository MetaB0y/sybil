---
tags: [testing, infrastructure]
layer: sequencer
status: current
last_verified: 2026-07-15
---

# Testing Strategy

Sybil's tests should make regressions cheap to catch without turning the test
suite into another distributed system. The default path is a small ladder of
increasing realism: pure deterministic tests, in-process API tests, process
restart tests, a tiny Docker smoke layer, then browser acceptance against an
assembled deployment.

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
- **Classify browser authority.** A browser acceptance run must say whether it
  is read-only, limited to a new disposable account, injecting a local fault,
  or acting as an operator. Missing fixtures block the run; they do not grant
  permission to mutate unrelated shared state.

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
  the API/history pair and a chain-11155111 Anvil process, deploys the exact
  accept-all Sepolia mock profile, runs the shared `sybil-client` `seed_book`
  fixture over real HTTP, and asserts exact trading conservation plus a real
  deposit/index/signed-withdrawal/relay/queue/delayed-finalize lifecycle. It
  runs the relay twice before indexing to pin restart idempotence. The default
  performs no proving; the older custody proof drill requires explicit
  `--with-escape`. Cleanup always runs `down -v`, and failures retain container
  logs under `target/itest-compose/`. `--dry-run` runs its assertion self-test
  without Docker.

The fixture is `SYB-247-v1`: BuyYes 0.60 × 1000 and BuyNo 0.50 × 2000. The
partially filled NO order pins the exact YES/NO clearing vector at 0.50/0.50;
matched volume is exactly 1000 share-units. Run id 0 is single-use on fresh
state. Persistent devnet smoke callers choose a new numeric `--run-id`, which
deterministically derives disjoint P256 seeds and replay nonces.

### 5. Browser and Computer-Use Acceptance

Browser acceptance tests what an assembled product communicates and permits;
it does not replace endpoint, signing, or state-machine tests. Playwright owns
repeatable browser-protocol checks such as virtual WebAuthn, focus containment,
responsive geometry, and an exact signed-order journey.

`frontend/web/computer-use/scenarios` is the tool-independent exploratory
layer. Each Markdown scenario has a checked seven-field header and fixed prose
contract: intent, preconditions, ordered steps, visible assertions, evidence,
cleanup, and stop conditions. Fixture names express capabilities rather than
hard-coded market/account IDs. Modes bound authority to `read-only`,
`disposable-account`, `controlled-fault`, or explicitly authorized `operator`
runs. The checker rejects implementation selectors so a different browser
agent can execute the same product contract.

Run `pnpm scenarios:check` in `frontend/web`; it is also part of
`just frontend-check`. Print the human catalog with `pnpm scenarios:list` or a
runner-facing catalog with `node scripts/check-computer-use-scenarios.mjs
--json`. Execution artifacts and redacted result records belong under
`target/computer-use`, not in source control. A missing screen or fixture is a
blocked product capability and should point to an issue; it is never silently
mocked into a pass.

### 6. Explicit Load and Isolation Tests

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

The second suite, `sybil-ws-load`, opens at least 100 public WebSocket streams
through the shared Rust client, stalls a configured subset of readers, and
checks every observed height across `lagged` reconnect/replay boundaries. It
samples health and metrics throughout the same interval and fails on missing
blocks, gaps/duplicates, failed recovery, excessive RSS/high-water growth,
actor queue depth, solve p99, or health p95. The ordinary public-devnet profile
measures concurrent fanout without requiring lag; the deterministic
backpressure profile uses a disposable fast-cadence stack so the TCP window
actually fills. See the [WebSocket load runbook](../../runbooks/websocket-load.md).

## Next Implementation Slice

1. Execute the P0 computer-use catalog against an exact-build disposable devnet
   and retain redacted result records; keep physical-device claims distinct
   from virtual-authenticator evidence.
2. Add a disposable, seeded API/history process fixture for a short automated
   historical-load smoke; keep capacity thresholds outside ordinary CI.
3. Add focused properties for sell-side position reservations and cancellation /
   expiry release invariants.
4. Move helpers into a separate test-support crate only if multiple crates begin
   sharing the same public fixtures.

This gives us better coverage without creating a new testing platform.
