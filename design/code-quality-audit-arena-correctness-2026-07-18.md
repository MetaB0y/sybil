---
tags: [audit, code-quality, arena, python, simulation, agents, operations]
layer: cross-cutting
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Arena data and experiment correctness audit — 2026-07-18

## Verdict

Arena's major ownership boundaries are sound: the exchange remains
authoritative for accounts, fills, blocks, and admission; Python owns agent
strategy and its experiment read model; and live forecasting is separate from
mechanical sizing. The concrete failures were at the edges of those
boundaries, where transport failure, simulated time, provider capability, or
API acceptance had been treated as if it were valid domain data.

This audit fixed those cases. Live agents now fail closed on unknown canonical
state, upstream LLM failures are explicit and evidence-preserving, server
admission policy is typed rather than copied, and simulation results use
observed simulated timestamps and strict per-day boundaries. Arena's 352 tests,
Ruff, API/OpenAPI tests, alert-rule tests, and documentation gates pass.

## Scope and invariants

Reviewed:

- `BaseAgent` state refresh, replay, pending-order, submission, and accounting
  behavior;
- live analyst/news/sizer evidence flow, provider failure policy, and paired
  experiment barriers;
- account identity recovery and live-run startup;
- simulation clock, external-price lookup, task supervision, day boundaries,
  and result persistence;
- exact Python SDK units and the server/Arena order-construction boundary;
- Arena metrics, status output, Grafana, and vmalert behavior; and
- deterministic iteration and run identity where it affects repeatability.

The review did not change solver policy, consensus bytes, proof code, or
production deployment state. It did not call a live LLM provider or run proof
generation.

The controlling invariants were:

- a strategy may act only on a successful canonical account and reservation
  view;
- replay repairs observations but never creates fresh side effects;
- API acceptance, not proposal or hook success, defines an accepted order;
- simulated output may use only information available at the represented
  simulated instant;
- one day's output must exclude every prior day's row;
- failed provider calls must not erase evidence or become a tight retry loop;
- local experiment budget is not evidence of provider credit;
- clients may discover admission constraints, but the sequencer remains
  authoritative; and
- a shared strategy layer must not silently enlarge an order and change risk.

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| AR-1 | High | `BaseAgent` continued into strategy logic after account/fill refresh failure and treated pending-order read failure as “none pending.” | Fixed: both unknown states suppress that block. |
| AR-2 | High | Proposed/rejected orders were logged as submitted; `max_blocks` advanced without API acceptance; a post-acceptance hook failure obscured the accepted write. | Fixed: only accepted batches affect acceptance telemetry and limits; hooks are isolated observers. |
| AR-3 | High | A transient account lookup during startup could replace a persisted bot identity and mint a newly funded account. | Fixed: replacement occurs only after an authoritative 404; availability failures abort. |
| AR-4 | High | Historical Polymarket lookup selected the first row on a calendar date, including observations after the exact simulation start. | Fixed: choose the latest valid row at or before the UTC start; malformed histories fail visibly to “no reference.” |
| AR-5 | High | Background bot/rebalancer failures could be hidden until a nominal simulation deadline and cancellation was not awaited coherently. | Fixed: named critical tasks race the deadline, exceptions invalidate the run, and cancellation is joined. |
| AR-6 | Medium | The clock used wall time, accepted invalid compression, and a sleep already in progress ignored later pauses. | Fixed with monotonic time, finite-positive validation, and pause-aware simulated sleeps. |
| AR-7 | High | Per-day results used inclusive block floors, cumulative trade logs, inferred block-height time, local-time/collision-prone names, and non-atomic writes. | Fixed with strict `after_block`, observed price-snapshot time, UTC UUID identity, day-only logs, and same-directory atomic replace. |
| AR-8 | High | Repeated LLM 401/402 failures left the container healthy while local budget gauges looked available; there was no classified provider state or alert. | Fixed under GitHub #192 with shared classification, bounded backoff, metrics, status, dashboard, and alerts. |
| AR-9 | High | Failed analyst calls consumed news evidence; paired A/B arms marked a batch consumed before the provider result. | Fixed with subscription requeue and paired lease/ack/retry semantics. |
| AR-10 | Medium | Transient analyst failures retried every block because only successful calls advanced the normal cadence. | Fixed: every provider attempt consumes the call interval. |
| AR-11 | Medium | A malformed lossy relevance-gate answer could be interpreted as “reject all,” permanently discarding already-seen evidence. | Fixed: only exact `NONE` or an in-range comma list is accepted; other output enters the visible fail-open provider path. |
| AR-12 | Medium | Arena copied no authoritative minimum-notional policy and repeatedly submitted server-rejected dust. | Fixed under GitHub #193 with `GET /v1/orders/policy`, an exact typed SDK contract, and central local suppression. |
| AR-13 | Low | Set iteration and second-resolution local run ids made equivalent simulations less reproducible and could overwrite output. | Fixed with sorted iteration and UTC UUID run ids. |

## Admission-policy boundary

The API now exposes one public, read-only `GET /v1/orders/policy` response:

- `min_order_notional_nanos`, serialized as an exact decimal string; and
- `share_scale`, checked by the Python wrapper against its canonical unit.

Live Arena fetches this policy before starting actors. `BaseAgent` applies the
same integer ceil-notional formula as sequencer admission to every ordinary
order. Below-minimum orders become local `below_min_notional` suppressions;
they never enter an HTTP rejection loop.

The framework intentionally does not increase quantity automatically. A
generic layer does not know whether the larger order exceeds cash, remaining
sell inventory, a target position, or an experiment's intended exposure.
Strategies may choose to construct a larger order themselves. Flash-liquidity
MM bundles remain exempt because their one-shot budget semantics are a
different protocol path.

Boundary coverage includes:

- very low price buys;
- tiny remaining sell inventory;
- insufficient cash without auto-upsizing;
- exactly-minimum notional;
- MM exemption;
- exact nanos above JavaScript's safe-integer range; and
- server/client share-scale drift.

Suppression is visible in durable sizer rejection reasons,
`sybil_arena_orders_suppressed_total`, `live.status`, and Grafana.

## Provider capability boundary

`ProviderCircuit` is the shared policy for the relevance gate and analyst
calls. It classifies authentication, credit, rate-limit, timeout, upstream,
and other failures. Authentication/credit and rate-limit failures use bounded
exponential backoff; transient calls remain eligible at their normal caller
cadence.

Provider state is deliberately independent of `llm_budget_remaining_usd`.
Arena now exports per-caller:

- classified failure totals;
- degraded state;
- last success and failure timestamps; and
- backoff-until time.

HTTP 401/402 triggers a critical alert immediately; persistent degraded state
triggers a warning after five minutes. Synthetic agents remain independent, so
the product can correctly report “exchange and synthetic flow alive, LLM
capability degraded” instead of collapsing those into one health bit.

Evidence ownership is explicit:

- gate failure passes candidates through because their URLs are already marked
  seen;
- ordinary analyst failure requeues the same articles ahead of newer evidence;
- paired experiment arms lease one exact evidence/reference snapshot;
- success acknowledges one arm; failure releases only that arm's lease; and
- the next paired batch cannot advance until both arms acknowledge.

## Simulation and result semantics

The simulation clock now measures elapsed real duration with `time.monotonic`.
Its UTC-naive simulation calendar remains an intentional dataset convention;
elapsed duration no longer depends on host wall-clock jumps.

External price lookup is point-in-time: it parses every timestamp, validates
price range, and chooses the latest observation no later than the exact
simulation start. A future row cannot seed the backtest.

Each run receives one UTC-and-UUID identity. Each day records the block height
that already existed before that day's actors started and includes only rows
strictly after it. Result `sim_time` comes from actual trader
`PriceSnapshot`s, not a fabricated linear block-height interpolation.
Background task failure aborts the run, and output uses a same-directory
temporary file plus atomic replacement.

## Accepted complexity and remaining trade-offs

No new issue was opened for these reviewed choices:

- `SyntheticFeed` uses a wall-clock resolution timestamp only in a small
  generic demo feed, not in the time-compressed experiment runner. Moving that
  class onto `SimulatedClock` would widen its API without a current consumer
  invariant.
- Status reads Prometheus from a short-timeout local HTTP endpoint on a
  best-effort basis. Trading does not depend on status rendering.
- The admission policy is process configuration fetched at Arena startup. A
  live API policy-reload protocol would need versioning and actor-wide refresh
  semantics; the current deployment changes it only through process restart.
- Provider-success means a transport/model response arrived. Output parsing
  quality remains a separate metric and calibration concern.

These are explicit trade-offs, not demonstrated defects. The next audit should
revisit them only with a concrete runtime requirement or executable witness.

## Verification

Passed:

```text
cd arena && uv run ruff check . && uv run pytest -q
# 352 passed

cargo test -p sybil-api --test api_integration \
  order_policy_exposes_the_active_admission_floor -- --exact
cargo test -p sybil-api --test route_policy
cargo test -p sybil-api --test openapi_drift
cargo test -p sybil-api-types --all-features

docker run --rm --entrypoint /bin/promtool ... \
  test rules tests/arena-liveness_test.yml
# SUCCESS

just arena-sdk-regen
just docs-check
```

The first all-feature API package invocation ran five process-restart tests
concurrently with other local verification and three health probes timed out
under contention. The complete five-test process-restart target passed in
4.25 seconds with `--test-threads=1`, matching the repository's known
child-process test isolation requirement. This is recorded rather than
misrepresented as a product failure.

No deployment, live-provider mutation, proof generation, protocol-byte change,
or solver-policy change occurred.
