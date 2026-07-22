---
tags: [runbook, testing, history, performance]
status: current
last_verified: 2026-07-13
---

# Historical-read isolation load test

This read-only [Goose](https://book.goose.rs/) test answers one narrow question:
does heavy historical query traffic make the sequencer slow or unavailable?
It is an architectural regression test first and a capacity test second.

The generator takes a short unloaded `/v1/health` baseline, then concurrently
requests account fills, account events, full equity, raw market prices, and
market candles through the public API. It continues to issue a separately
named `/v1/health` control request throughout the loaded phase. Health reads
the sequencer's atomic chain-status snapshot, while the historical requests
should authorize from the API read model and execute in `sybil-history`.
Every response body is consumed before that virtual user begins its next
request. This preserves HTTP connection reuse and makes the achieved request
rate include complete bounded history-response transfer rather than measuring
headers followed by canceled bodies and connection churn.
Goose's built-in per-request latency ends when response headers arrive; the
virtual-user cadence and achieved throughput include the full body read. The
pass/fail latency budget intentionally applies only to the small `/v1/health`
control response.

The run fails when:

- any HTTP request fails;
- too few real health or history samples were collected;
- loaded health p95 exceeds the absolute ceiling; or
- loaded health p95 grows too far above its pre-load baseline.

Goose also writes its normal HTML report, including per-route throughput and
tail latency. Coordinated-omission mitigation is enabled so a stalled virtual
user does not make the result look healthier than it was.

## Prerequisites

- Run a stack with `sybil-history` configured and caught up.
- Use `SYBIL_DEV_MODE=false` when validating owner-auth isolation.
- Create a read bearer token belonging to the selected account. A service
  token can make the routes succeed, but it bypasses the owner-token lookup
  and therefore weakens this particular test.
- Select a market id. The market may have sparse history, but a populated
  account and market produce the most representative response sizes.
- Run the load generator on another machine for capacity conclusions. A local
  run is useful as a smoke test but includes generator CPU/network contention.

The test performs GET requests only. It does not create accounts, place
orders, advance blocks, or otherwise mutate the target.

## Run

From the repository root:

```bash
export SYBIL_LOADTEST_HOST=https://devnet.example
export SYBIL_LOADTEST_ACCOUNT_ID=42
export SYBIL_LOADTEST_BEARER_TOKEN='<owner read token>'
export SYBIL_LOADTEST_MARKET_ID=7

just history-load --users 64 --hatch-rate 16 --run-time 2m \
  --report-file target/history-load-report.html
```

The binary supplies conservative smoke defaults when Goose flags are omitted:
32 users, 8 users/second startup, a 30-second loaded phase, and
`target/history-load-report.html`. Standard Goose CLI flags override those
defaults.

Never put the bearer token in a command line, report name, shell history, or
committed configuration. The binary reads it only from the environment and
does not print it.

## Thresholds

| Environment variable | Default | Meaning |
|---|---:|---|
| `SYBIL_LOADTEST_BASELINE_SAMPLES` | 30 | Sequential pre-load health probes |
| `SYBIL_LOADTEST_BASELINE_INTERVAL_MS` | 20 | Delay between baseline probes |
| `SYBIL_LOADTEST_MIN_HEALTH_SAMPLES` | 100 | Minimum real loaded health responses |
| `SYBIL_LOADTEST_MIN_HISTORY_SAMPLES` | 500 | Minimum real historical responses |
| `SYBIL_LOADTEST_MAX_HEALTH_P95_MS` | 250 | Absolute loaded sequencer-health p95 ceiling |
| `SYBIL_LOADTEST_MAX_HEALTH_P95_INCREASE_MS` | 100 | Allowed p95 increase over baseline |

Tune thresholds deliberately for a larger test profile; do not loosen them
merely to make a saturated undersized host pass. Keep the Goose HTML report
with the machine/profile details so results remain comparable.

## Reading a failure

- History request failures with healthy control latency indicate history
  capacity, authorization, routing, or projection availability—not sequencer
  coupling.
- Health p95 growth with successful history reads means the isolation is
  incomplete at the API runtime, actor mailbox, or shared-host CPU/disk layer.
- Both request classes slowing on a same-host run may be host saturation. Run
  the generator off-host, inspect API/history CPU and redb latency, then repeat.
- Too few samples means the profile was too short or too small to support a
  regression verdict; increase duration/users instead of lowering the minimum
  until it becomes meaningless.

This is not a deployment smoke gate and does not run in the default fast test
suite. Run it after changes to history routing/auth/storage, before capacity
claims, and against release builds.
