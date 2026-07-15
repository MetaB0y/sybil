---
tags: [runbook, testing, websocket, performance]
status: current
last_verified: 2026-07-15
---

# WebSocket fanout and recovery load test

`sybil-ws-load` answers two related questions about the public
`GET /v2/blocks/ws` stream:

1. Can the target hold at least 100 concurrent subscribers without material
   process-memory, sequencer-mailbox, solve-latency, health-latency, or block-
   cadence regression?
2. When an intentionally slow reader overruns the 64-block broadcast window,
   does it receive `lagged`, reconnect from `last_sent_height + 1`, replay a
   contiguous prefix, receive `replay_complete`, and resume without gaps or
   duplicates?

The generator uses the shared Rust `sybil-client`, performs only public GET and
WebSocket operations, and never creates accounts, submits orders, or advances
blocks. Slow-cohort clients stop reading for one configured stall, then read at
full speed so any `lagged` envelope and reconnect replay can catch the live
head. It writes a machine-readable JSON report even when the final threshold
verdict fails.

## Why there are two profiles

At the deployed 10-second block interval, even a reader that pauses for minutes
usually fits many small block envelopes in the kernel TCP receive window. That
is useful for measuring real fanout capacity but does not deterministically
exercise the server-side 64-block lag branch.

Use both profiles for acceptance:

- **Normal-cadence capacity:** target the release stack, keep
  `SYBIL_WS_LOADTEST_REQUIRE_LAG=false`, and run long enough to observe at least
  70 committed blocks. Run the generator off-host for a capacity claim.
- **Fast-cadence recovery:** target a disposable local/isolated API with the
  fastest block interval the host can sustain, require lag, and stall the slow
  readers long enough to fill their TCP window. This profile validates the
  protocol recovery path; it is not a production throughput claim.

Do not use the fast-cadence profile against a shared devnet. Changing the block
interval or seeding a disposable target is target setup, not work performed by
the load binary.

## Disposable recovery profile

Start an in-memory API on an unused port. The connection cap must exceed the
subscriber count:

```bash
RUST_LOG=sybil_api=info,matching_sequencer=info \
SYBIL_BLOCK_INTERVAL_MS=20 \
SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS=150 \
SYBIL_RECENT_BLOCK_CACHE_CAPACITY=20000 \
SYBIL_WS_CLIENT_IDLE_TIMEOUT_MS=300000 \
cargo run -p sybil-api -- --dev-mode --port 3101
```

In another shell:

```bash
export SYBIL_WS_LOADTEST_HOST=http://127.0.0.1:3101
export SYBIL_WS_LOADTEST_SUBSCRIBERS=100
export SYBIL_WS_LOADTEST_SLOW_SUBSCRIBERS=10
export SYBIL_WS_LOADTEST_SLOW_READ_STALL_MS=150000
export SYBIL_WS_LOADTEST_RUN_SECONDS=180
export SYBIL_WS_LOADTEST_MIN_BLOCKS=6000
export SYBIL_WS_LOADTEST_REQUIRE_LAG=true
export SYBIL_WS_LOADTEST_REPORT_FILE=target/ws-load-recovery.json

just ws-load
```

The stall must end early enough for queued frames, the final lag envelope, and
reconnect replay to drain before the run ends. If the machine has an unusually
large TCP autotuning window and no lag is observed, increase the stall and run
time together before increasing cadence. If health times out before subscribers
open, the target cadence is unsustainable; slow it down. Preserve the failed
report: a missing lag event is a failed recovery-profile verdict, not a reason
to disable the assertion.

The disposable target's recent-block cache must cover the full fast-cadence
run. Without that override, its default 100-block hot window is exhausted long
before a stalled client reconnects; an in-memory target has no durable archive
fallback and will correctly fail replay rather than invent missing blocks.
The idle-timeout override must likewise exceed the stall: the normal 90-second
production setting closes a client that has sent no Pong or other frame before
the backpressure branch can be observed. Both overrides are isolated target
setup; do not carry them into a shared deployment.

## Normal-cadence capacity profile

Use a release stack and run the generator from another host. At 10 seconds per
block, 70 blocks needs at least 12 minutes plus connection startup:

```bash
export SYBIL_WS_LOADTEST_HOST=https://devnet.example
export SYBIL_WS_LOADTEST_SUBSCRIBERS=100
export SYBIL_WS_LOADTEST_SLOW_SUBSCRIBERS=10
export SYBIL_WS_LOADTEST_SLOW_READ_STALL_MS=60000
export SYBIL_WS_LOADTEST_RUN_SECONDS=780
export SYBIL_WS_LOADTEST_MIN_BLOCKS=70
export SYBIL_WS_LOADTEST_REQUIRE_LAG=false
export SYBIL_WS_LOADTEST_REPORT_FILE=target/ws-load-capacity.json

just ws-load
```

Coordinate this run with the devnet operator. The generator is read-only, but
100 long-lived sockets and repeated health/metrics probes are intentional load.

## Default thresholds

| Environment variable | Default | Meaning |
|---|---:|---|
| `SYBIL_WS_LOADTEST_SUBSCRIBERS` | `100` | Required concurrent public streams; values below 100 are rejected |
| `SYBIL_WS_LOADTEST_SLOW_SUBSCRIBERS` | `10` | Readers held in the initial no-read stall |
| `SYBIL_WS_LOADTEST_RUN_SECONDS` | `60` | Loaded observation interval |
| `SYBIL_WS_LOADTEST_SLOW_READ_STALL_MS` | `45000` | One initial no-read interval for the slow cohort; must be shorter than the run |
| `SYBIL_WS_LOADTEST_SAMPLE_INTERVAL_MS` | `250` | Health and metrics sampling interval |
| `SYBIL_WS_LOADTEST_BASELINE_SAMPLES` | `20` | Unloaded health/metrics samples before connections |
| `SYBIL_WS_LOADTEST_BASELINE_INTERVAL_MS` | `100` | Delay between baseline samples |
| `SYBIL_WS_LOADTEST_MIN_BLOCKS` | `70` | Minimum committed-height advance under load |
| `SYBIL_WS_LOADTEST_REQUIRE_LAG` | `true` | Require at least one slow client to lag and recover |
| `SYBIL_WS_LOADTEST_MAX_RSS_GROWTH_MIB` | `128` | Maximum loaded RSS growth over baseline |
| `SYBIL_WS_LOADTEST_MAX_HWM_GROWTH_MIB` | `128` | Maximum process high-water growth over baseline |
| `SYBIL_WS_LOADTEST_MAX_ACTOR_QUEUE_DEPTH` | `128` | Maximum sampled sequencer mailbox backlog; well below its 1,000 warning threshold |
| `SYBIL_WS_LOADTEST_MAX_SOLVE_P99_MS` | `100` | Absolute solve p99 ceiling |
| `SYBIL_WS_LOADTEST_MAX_SOLVE_P99_INCREASE_MS` | `50` | Allowed solve p99 increase over baseline |
| `SYBIL_WS_LOADTEST_MAX_HEALTH_P95_MS` | `250` | Absolute loaded health p95 ceiling |
| `SYBIL_WS_LOADTEST_MAX_HEALTH_P95_INCREASE_MS` | `100` | Allowed health p95 increase over baseline |

The binary fails if any required `/metrics` series is missing. It checks both
RSS and process high-water because RSS can fall before the final sample. Solve
p99 uses the exported `sybil_solve_time_seconds{quantile="0.99"}` summary; it
does not invent histogram buckets.

## Reading the report

The report records configuration, baseline and loaded metrics, and one row per
subscriber. Every lag event must be followed by a reconnect that reaches
`replay_complete`. Every block seen by each subscriber is checked against the
next exact expected height across connections, so a successful report proves
no observed gap or duplicate.

- High RSS/high-water with flat actor/solve metrics points to fanout/socket
  retention in the API process.
- Actor queue or solve p99 growth means stream load is coupling back into block
  production; inspect host saturation before changing a threshold.
- Healthy metrics with connection failures usually indicate the shared public
  stream cap, proxy limits, file-descriptor limits, or TLS termination.
- A retention gap means the recovery profile did not retain enough canonical
  blocks for its slowest client. Increase target retention or shorten the test;
  do not treat a cold resync as successful reconnect recovery.

This suite is explicit and is not part of fast CI. Store the JSON artifact with
the target revision, machine/profile, and whether the generator ran off-host.
