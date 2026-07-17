# Public CLOB-depth corpus

Date: 2026-07-17

Status: accepted development benchmark; external resting-depth evidence with
explicitly synthetic batch arrivals and maker capital.

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
