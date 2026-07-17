# Exact economic-connectivity decomposition

Date: 2026-07-17

Status: accepted research solver and benchmark instrumentation.

## Experiment ECD-001 — exact component router

### Hypothesis

The retained-cash problem is exactly separable after joining every pair of
markets connected by a categorical group, one spanning order, or one shared MM
budget. A conditional order also joins its traded and condition markets.
Solving the remaining components independently should improve
fragmented-book latency and finite-iteration quality without changing
connected-book behavior.

This differs from `DecomposedSolver`: shared MM budgets are coarsened into one
component instead of being split and iteratively coordinated.

### Implementation

`ExactComponentSolver<S>` uses union-find to build the connectivity graph and
wraps any inner `Solver`. It preserves complete MM constraints and budgets,
orders, groups, and ordinary integer landing. Connected books delegate
directly. The experiment runner records component count and largest-component
market, order, and MM shares.

Source during development: working change `ntrzzsxz`, parent
`c9c5ac72b8fb`.

### Broad development comparison

The price-pacing development matrix was reduced to a paired comparison between
the monolithic HiGHS-backed pacing bundle and the exact wrapper: 59 books,
118 rows, with market-like, flash, numerical, and 1/4/16-MM scaling slices.
Every row was solver-successful, verifier-valid, and fingerprint-consistent.

```bash
jq '
  .solvers["exact-components-pacing-bundle"] =
    (.solvers["pacing-bundle"]
      | .kind = "exact-components-pacing-bundle"
      | .label = "Exact connectivity components around pacing bundle")
  | .experiments |= map(
      .solvers = ["pacing-bundle", "exact-components-pacing-bundle"])
' benchmarks/solver/protocol-price-pacing-development.json \
  > /tmp/protocol-exact-components.json
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol /tmp/protocol-exact-components.json \
  --source-revision exact-components-working-copy \
  --output-dir /tmp/exact-components-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/exact-components-development
```

| Exact components | Cases | Median wrapped/monolithic runtime | Retained-objective result |
|---:|---:|---:|---|
| 1 | 28 | 0.997× | 28 identical |
| 2 | 25 | 0.992× | 25 identical |
| 4 | 3 | 0.223× | 2 identical, 1 improved after the monolith capped |
| 16 | 3 | 0.063× | all improved by `$0.061`–`$0.419` after the monolith capped |

Across the whole paired matrix the wrapper's P50/P95/max latency was
`23.78/334.08/587.27 ms`, versus `90.55/341.30/582.52 ms` monolithically.
It had 59/59 successes and one capped row versus 59/59 and four capped rows.
Retained objective was identical in 55 pairs and better in four; it was never
worse. The large median gain is intentionally conditional on this matrix's
balanced 4- and 16-component scaling slices, not a traffic-distribution claim.

### Replay counterexample and routing rule

A five-solver, 720-row paired replay compared raw decomposition with the
monolithic bundle on all 144 sequencer books/budget points. Economics were
identical in all 144 pairs, but the wrapper was 1.636× slower on the 20
fragmented rows. Those books had only a tiny post-resolution tail: the largest
component held a median 94.3% of orders and every MM.

The router now decomposes only when the largest component contains at most 80%
of orders. This is a performance policy, not an approximation: unbalanced
books delegate to the unchanged monolithic problem. Replay protocol v3 then
replaced the raw structural bundle with the routed variant, retaining the same
four solvers and 576 rows.

The final replay was 576/576 successful and verifier-valid. The routed bundle
was 144/144 with no caps and P50/P95 latency `4.56/227.26 ms`; its allocation,
welfare, retained objective, and termination matched the prior monolithic
bundle rows. Timing across separate process runs is not treated as a replay
speed claim.

### Benchmark signal discovered

The new connectivity coverage table showed:

| Replay regime | Unique books | Fragmented | Largest-component orders P50 |
|---|---:|---:|---:|
| Standard flow | 20 | 0 | 100.0% |
| Grouped news | 20 | 0 | 100.0% |
| Mid-resolution | 20 | 10 | 94.3% |
| Dense stress | 12 | 0 | 100.0% |

Thus the lifecycle replay is useful for connected-book regressions but is a
poor standalone optimization signal for fragmented scaling. Extended
autoresearch-style work must keep the balanced-component synthetic slices and
report topology coverage beside replay results.

### Decision

Keep the generic exact router and compact topology metrics. Keep the 80%
delegation rule until a wider traffic corpus supports a different threshold.
Do not replace `DecomposedSolver`: it explores the distinct trade of finer
parallelism through approximate shared-budget coordination.

Revisit when deployed or multi-seed captures establish a real distribution of
component balance, or when per-component setup/landing cost changes
materially.

## Experiment ECD-002 — broad production-candidate comparison

### Hypothesis

The exact router should remove the monolithic bundle's balanced-component
scaling tail while preserving its stronger retained-objective convergence.
If so, `ExactComponentSolver<PacingBundleSolver>` is a credible replacement
candidate for production RC-FW, subject to a separately frozen untouched-seed
evaluation.

### Development result

Source revision `6b4248f4a3c6` was evaluated on all 126 cases from the pacing
development protocol. The paired matrix compared RC-FW, the monolithic bundle,
and the exact-component bundle for 378 complete, fingerprint-consistent rows:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol /tmp/protocol-bundle-production-candidate.json \
  --source-revision bundle-production-candidate-main-6b4248f4 \
  --output-dir /tmp/bundle-production-candidate --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/bundle-production-candidate
```

| Metric | RC-FW | Monolithic bundle | Exact bundle |
|---|---:|---:|---:|
| Success | 125/126 | 126/126 | 126/126 |
| Iteration caps | 28 | 6 | 2 |
| P50 / P95 / max | 124 / 455 / 2,214 ms | 80 / 553 / 2,244 ms | 29 / 469 / 1,724 ms |
| Retained gap P95 / max | 0.078 / 20.871% | 0 / 0.0027% | 0 / 0.0002% |
| Landing L1 max | 41.530% | 3.069% | 3.069% |

The balanced 10,000-order slice fell from `1.725 / 2.240 s` bundle P50/P95
to `0.413 / 0.623 s`; the 16-component slice fell from `215 / 218 ms` to
`14 / 15 ms`. Connected numerical-range cases remained the important tail and
were essentially unchanged. Exact bundle P95 was 3% slower than RC-FW, so the
result is a broad Pareto judgment, not strict dominance.

Decision: promote the exact bundle to the preferred production candidate, but
do not change the default from development evidence. Freeze
`benchmarks/solver/protocol-bundle-promotion-v1.json` and the implementation
before evaluating its untouched seeds once.
