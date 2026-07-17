# Public CLOB-depth corpus

Date: 2026-07-17

Status: accepted development benchmarks; external resting-depth and
second-resolution taker-flow evidence with explicitly synthetic maker capital.

## Experiment PDC-001 — raw snapshots as batch workloads

### Hypothesis

A compact cross-category sample of public Polymarket order books would add
realistic price, quantity, depth, and market-group shapes that the independent
scenario generator and sequencer-sim do not provide.

### First design and rejection

The first corpus converted every retained YES/NO bid and ask directly into a
one-hot Sybil order. It selected one volume-ranked eligible event in each of
six tag buckets, retained at most 20 near-touch levels per token side, and
preserved every market in a selected NegRisk event. Event-local cases added
two labelled synthetic shared-budget MMs; a raw and a budgeted cross-event
portfolio completed the eight cases.

The 60-row protocol ran correctly but only 20 rows were non-empty. Five of the
six event books cleared zero quantity for all four solvers. This is not a
solver failure: a continuous CLOB has already removed crossing orders, so an
instantaneous resting snapshot is usually a no-trade FBA input.

Decision: reject raw event snapshots as the primary clearing workload. Keep one
raw portfolio as a topology/no-trade control. Do not rediscover this by adding
more categories or depth levels; it is a structural mismatch between a
continuous resting book and a batch.

## Experiment PDC-002 — depth-calibrated synthetic arrivals

### Hypothesis

Preserving the observed resting depth while adding a small, explicit batch
shock should exercise realistic price/quantity geometry without claiming that
the unobserved incoming flow or maker capital is public evidence.

### Frozen corpus

Capture command:

```bash
uv run scripts/benchmarks/capture_polymarket_depth.py \
  --output benchmarks/solver/corpora/polymarket-clob-depth-20260717-v1.msgpack \
  --manifest benchmarks/solver/corpora/polymarket-clob-depth-20260717-v1.manifest.json \
  --corpus-id polymarket-clob-depth-20260717-v1
```

The final artifact is 598,262 bytes with BLAKE3
`22bc4af085c1718059779fe1a2ae9f1e8a6dad8611608a613aa8f023c5295eaa`.
It contains 50 markets and eight cases. Source event IDs are `411239`,
`710524`, `688327`, `287395`, `630845`, and `626857`; the checked-in manifest
retains the titles, tags, CLOB token IDs, condition IDs, source book hashes and
timestamps, level counts, selection rules, and transformation description.

For each market in a shocked case, the transform alternates between a
BuyYes/SellNo and SellYes/BuyNo pair. Each arrival sweeps at most the first
three observed opposing levels plus half the touch quantity. Two synthetic MMs
quote at the observed touch and one tick wider, share capital across every
market in the case, and use the sum of worst-case quote capital as the base
budget. The protocol evaluates both `0.1×` and `1×` that base. The raw
cross-event case has neither arrivals nor MMs.

### Baseline result

Source during development: working change `pmsxlutn`, parent
`37f2f6f78137`.

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-public-depth-development.json \
  --source-revision public-depth-arrivals-working-copy \
  --output-dir /tmp/public-depth-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/public-depth-development
```

The matrix was complete and fingerprint-consistent. All landed results were
verifier-valid. Fifty-nine of 60 solver rows succeeded: Clarabel returned
`InsufficientProgress` on the loose-budget 50-market connected portfolio, with
primal residual `1.38e-8`, dual residual `4.00e-13`, and absolute gap
`$0.872486`. The tight event budgets reached 99.69% median MM utilization
across the reported rows, while the loose event budgets were 49.75%; the
connected portfolio was 98.49% versus 43.54%. Thus the two points exercise
materially different capital regimes.

| Solver | P50 / max latency | Median / max retained gap | Notable tail |
|---|---:|---:|---|
| LP-SLP | 4.54 / 19.27 ms | 0 / 0.0296% | fastest landed reference |
| RC structural | 6.28 / 85.57 ms | 0.0015 / 2.3825% | 2.3825% landing loss on culture |
| Exact bundle structural | 6.49 / 93.30 ms | 0 / 2.3825% | same landing counterexample |
| Clarabel Quasi | 8.70 / 53.62 ms | 0 / 0.0401% | 14/15; one numerical failure |

The raw 50-market portfolio had 27 exact economic components and only 17.9% of
orders in the largest one. Every shocked event and the shared-budget portfolio
was connected. This is a useful complement to the mostly connected lifecycle
replay.

### Signal and next experiment

The corpus exposes two independent signals. Clarabel has a concrete
large-connected-book `InsufficientProgress` reproducer, suitable for a bounded
upstream/fork investigation. Separately, the continuous-to-integer boundary is
backend-independent: both retained-cash routes can report essentially zero
continuous certificate gap and then lose 2.3825% during landing; on the
50-market loose-budget portfolio both discard `$2.953372` of objective. A
domain-specific integer face projection or repair is the next preferred solver
experiment because it can benefit more than one continuous backend.

### Evidence boundary

The public observations are anonymous aggregated resting price levels,
quantities, token pairing, tick sizes, event categories, and group structure.
They do not reveal maker identity, maker budget, order arrival time, cancelled
liquidity, or a Sybil admission sequence. The synthetic arrivals deliberately
condition on observed depth and therefore are not independent samples.

One time snapshot is vulnerable to event selection and time-of-day effects.
Use this corpus for development regression and geometry, not traffic-frequency
claims. Revisit with separately versioned captures at multiple times and with
privacy-reviewed solver-boundary replays; never overwrite this frozen artifact.

## Experiment PDC-003 — observed public taker-flow windows

### Hypothesis

Replacing the depth-conditioned synthetic event shocks with compact public
taker-flow windows should improve real-world price, size, side, and short-burst
geometry without making the corpus large or retaining trader identities.

### Capture and projection

Source during development: working change `potkwwom`, parent
`9c8e85607d5c`.

```bash
uv run scripts/benchmarks/capture_polymarket_depth.py \
  --arrival-source observed-trades \
  --output benchmarks/solver/corpora/polymarket-clob-flow-20260717-v1.msgpack \
  --manifest benchmarks/solver/corpora/polymarket-clob-flow-20260717-v1.manifest.json \
  --corpus-id polymarket-clob-flow-20260717-v1
```

The artifact was captured at `2026-07-17T14:37:06+00:00`, is 599,688
bytes, and has BLAKE3
`8d2e113c70f8c70b2e2d39d4abb5635a56c220d478c027a3bd561555a954fc59`.
It retains the same 50 markets and eight-case shape as PDC-002. The Data API
rows are immediately projected to condition, outcome token, taker side, price,
size, and second-resolution timestamp. Wallet and profile fields are neither
represented in the in-memory projection nor written to the manifest or corpus.
A transaction hash is used only as an in-memory deterministic sort tie-breaker.

For each event, the transform chooses the aligned one-second bucket with the
most trades in a 24-hour lookback, breaking ties by distinct markets, total
quantity, and recency:

| Category | Trades | Markets | Buy / sell | Shares |
|---|---:|---:|---:|---:|
| Politics | 23 | 6 | 23 / 0 | 1,164.092 |
| Sports | 18 | 2 | 17 / 1 | 1,424.012 |
| Crypto | 13 | 2 | 2 / 11 | 7,070.820 |
| Economics | 20 | 5 | 20 / 0 | 103,562.326 |
| Technology | 13 | 3 | 13 / 0 | 2,669.526 |
| Culture | 32 | 6 | 29 / 3 | 6,115.407 |

The six event cases preserve those observed windows. The raw portfolio still
has no arrivals or MMs. The budgeted portfolio is an explicitly synthetic
cross-event composition of the six asynchronous windows. Both the maker
identities and maker capital remain synthetic.

### Budget calibration

A 70-row retained-solver sweep evaluated `0.01×`, `0.025×`, `0.05×`, `0.1×`,
and `1×` maker budgets across all seven budgeted cases. All rows landed and
verified.

| Budget | Median max utilization | Max retained gap | Decision |
|---|---:|---:|---|
| `0.01×` | 97.63% | 31.3854% | reject; two RC iteration caps |
| `0.025×` | 83.60% | 0.0954% | reject; culture RC iteration cap |
| `0.05×` | 83.79% | 0.001052% | accept; all retained solvers converge |
| `0.1×` | 41.89% | 0.000082% | reject as a redundant mild point |
| `1×` | 4.19% | 0.000000% | accept as the slack control |

Thus the fixed protocol uses `0.05×` and `1×`. This was selected before the
final four-solver matrix rather than after comparing solver winners.

### Baseline result

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-public-flow-development.json \
  --source-revision public-observed-flow-final-working-copy \
  --output-dir /tmp/public-flow-final --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/public-flow-final
```

The matrix was complete and fingerprint-consistent. All 60 rows succeeded and
every landed result was verifier-valid.

| Solver | P50 / max latency | Median / max retained gap | Notable tail |
|---|---:|---:|---|
| LP-SLP | 2.84 / 49.76 ms | 0 / 0.1096% | 2/15 reached the SLP cap |
| RC structural | 6.71 / 84.77 ms | 0 / 0.001052% | 3.925% landing movement on tight culture |
| Exact bundle structural | 5.99 / 83.99 ms | 0 / effectively 0 | 0.499% maximum landing movement |
| Clarabel Quasi | 8.06 / 55.61 ms | 0 / 0.0103% | 15/15 available |

The tight culture window is a useful multi-metric discriminator. RC structural
lands `$62.0906` net welfare with a 0.001052% retained gap and 3.925% target
movement; the exact bundle and Clarabel land `$62.0916`, with the bundle moving
0.499%. LP has zero welfare gap against the landed welfare reference but a
0.1096% retained-cash gap. No single scalar captures those trade-offs.

### Decision and evidence boundary

Accept the corpus and protocol as compact development signal. It found a
retained-cash convergence/landing tail that the synthetic public-depth shock
did not isolate, while preserving the raw 27-component topology control.

Do not call it Sybil traffic or a live batch replay. Public timestamps are only
one second, the retained book snapshot was captured later than the selected
historical windows, and the densest-window rule deliberately over-samples burst
activity. One capture is vulnerable to event and time-of-day selection.
Revisit with independently frozen times and privacy-reviewed solver-boundary
replays; never overwrite this artifact.
