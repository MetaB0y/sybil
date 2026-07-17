# Solver experiments

This directory owns the reproducible solver-evaluation protocol. It is for
empirical claims, not for criterion-style microbenchmarks or a single ad-hoc
`matching-sim` comparison.

## Integrity rules

- Each versioned protocol fixes scenarios, solvers, seeds, budget points, iteration
  limits, time limits, primary metrics, exclusions, and interval construction
  before the full run.
- Every declared run produces one JSONL record. Panics, numerical failures,
  empty results, timeouts, verifier failures, and iteration caps are retained.
- All solvers in a run receive a byte-identical generated `Problem`; the
  analysis rejects fingerprint mismatches and duplicate/missing run keys.
- Protocol v1 ranks integer verifier-recomputed net welfare. Protocol v2 adds
  the shifted retained-cash objective as its primary objective, evaluates it on
  every landed allocation, and keeps integer net welfare as the protocol and
  approximation metric.
- Development protocols are checked in when they define a reusable stress
  design, but their rows are never promoted to held-out evidence. In
  particular, `protocol-pacing-development.json` uses seeds 16000--19004:
  16000--18403 supported pacing-bundle development, while the later
  19000--19004 range is reserved for the market-like snapshot workload.
- LP is a production reference, not asserted to be a global optimum under the
  exact bilinear MM-budget model. MILP is called exact only when SCIP reports a
  proven optimum.
- Research solvers do not silently substitute LP on failure. Mathematically
  explicit delegation remains possible only when the requested objective
  reduces to LP, such as a no-MM problem or Conic `Linear` mode.
- Synthetic profiles are structural stress tests. They are not described as
  calibrated real order flow; the repository currently has no frozen replay
  dataset suitable for that claim.

## Run

Protocol v1 is the retained historical evaluation and must be reproduced from
the frozen source revision recorded beside its results; its removed legacy
solver names are intentionally unavailable on current `main`. Protocol v2 is the current
two-sided flash-liquidity evaluation; its full-run seeds start at 50000 and are
kept separate from development seeds below 30000.

First exercise every suite and solver path with development-only seeds. In v2,
`--smoke` maps the declared 50000+ ranges onto disjoint 20000+ development
seeds; it does not consume the evaluation books:

```bash
cargo run --release -p matching-sim --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-v2.json \
  --output-dir /tmp/solver-smoke --smoke --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/solver-smoke --allow-incomplete
```

For a publishable run, freeze the implementation in its own `jj` change and
pass that immutable commit:

```bash
cargo run --release -p matching-sim --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-v2.json \
  --source-revision <implementation-commit> \
  --output-dir benchmarks/solver/results/<date>-v2
python3 scripts/benchmarks/analyze_solver_experiments.py \
  benchmarks/solver/results/<date>-v2
```

The runner copies the protocol beside `results.jsonl` and writes machine and
toolchain metadata. The analyzer validates completeness, then creates
`summary.json`, `summary.csv`, `summary.md`, and deterministic SVG figures.

## Pacing-bundle development protocol

`protocol-pacing-development.json` compares the certified RC-FW production
solver, the experimental fully corrective pacing bundle, five-step LP-SLP,
corrected-epigraph Clarabel QuasiFisher, and a deliberately budget-blind LP
negative control. It is designed to answer two separate scaling questions
rather than confound them:

- order count grows from 80 to 10,000 with exactly two market makers;
- pacing dimension grows from 1 to 16 market makers with 2,000 orders;
- tight two-sided flash ladders exercise both MM bids and complementary-buy
  reductions of MM asks;
- a market-like snapshot combines long-tailed resting flow with the live
  integration's dollar-sized, group-safe Buy YES/Buy NO flash quotes;
- random, concentrated, heavy-tailed numerical, and tiny reference books keep
  the structural stress from becoming one hand-picked family.

The detailed work metrics include P90/P95/P99/max wall time, landed welfare
mean/P50/P95/max, retained-objective tails, LP-oracle calls
and time, restricted-master steps and time, active bundle atoms, certified
continuous gap, integer landing loss, L1 target movement, budget-repair counts,
and the landed minting-duality gap
`|C_0(D) - p·D|`. The last metric detects allocations whose prices no longer
support their post-processed fill vector, even if ordinary limit and budget
checks still pass.

The 14 July retained run and the investigation of the former 67.9% landing gap
are documented in
`design/pacing-bundle-landing-tail-study-2026-07-14.md`. Its lexicographic
face-preserving landing and price-support gate are materially safer but make the
10,000-order latency tail an explicit open engineering problem.

Run every declared development row with:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-pacing-development.json \
  --source-revision <working-or-frozen-revision> \
  --output-dir /tmp/solver-pacing-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/solver-pacing-development
```

This protocol is intentionally diagnostic. Any later paper comparison must
freeze the implementation and a new untouched seed range before running it;
changing seeds, dropping failed books, or selecting a favorable budget point
after seeing this development matrix would invalidate that comparison.

## Direct price-pacing development protocol

`protocol-price-pacing-development.json` adds the exact price-side Clarabel
dual to a compact 59-case comparison against RC-FW, pacing bundle, and the
fill-side Clarabel reference. It reuses the market-like and tight flash
families, adds numerical-range stress, and isolates pacing dimension at 1, 4,
and 16 MMs. Seeds `19100..19602` are development-only.

The protocol exists to separate three outcomes that an aggregate objective can
hide: continuous certificate quality, backend availability, and loss incurred
while selecting and landing an integer point on a degenerate optimal face.
Full results and rejected precursor methods are recorded in
`design/solver-experiments/price-pacing-dual.md`; the raw development artifact
is reproducible but is not held-out evidence.

Run it with:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-price-pacing-development.json \
  --source-revision <working-or-frozen-revision> \
  --output-dir /tmp/price-pacing-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/price-pacing-development
```

## Structural-oracle development protocol

`protocol-structural-oracle-development.json` isolates one implementation
choice inside both retained-cash algorithms: the reusable HiGHS matching LP
versus an exact domain-specific price sweep with analytical primal recovery.
It declares 61 cases per solver and retains every recovery, certificate,
landing, budget-fixed-point, and verifier failure.

The workload crosses market-like order flow, a six-point tight flash-budget
sweep, wide numerical ranges, and fixed-order-count 1/4/16-MM scaling. This
makes it useful for detecting objective, availability, latency-tail, and
degenerate-face regressions together. It is still synthetic development
evidence, not a substitute for sequencer-boundary replay.

Run it with:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-structural-oracle-development.json \
  --source-revision <working-or-frozen-revision> \
  --output-dir /tmp/structural-oracle-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/structural-oracle-development
```

The exact derivation, rejected marginal-face selectors, counterexample seed,
and current development result are recorded in
`design/solver-experiments/structural-price-sweep-oracle.md`.

## Protocol v2 suite structure

- **Random quality:** neutral slack/tight controls and concentrated tight books,
  with budgets calibrated from the unconstrained MM limit-value.
- **Two-sided flash sweep:** deterministic bid/ask ladders sharing capital
  across markets, including the sell-to-complementary-buy reduction, over six
  budget ratios.
- **Flash scaling:** 48, 400, and 2,000 total orders across independently seeded
  books.
- **Numerical stress:** heavy-tailed whole-share quantities and wide budget
  ranges.
- **Exact reference:** small instances against a proven SCIP MIQCQP result when
  SCIP reports optimality.
- **Ablation:** retained cash versus the forced-spend no-cash Fisher objective.

The v2 budget-blind LP is deliberately permitted to violate MM capital and is
reported as an infeasible negative control. The LP-SLP, retained-cash FW,
Clarabel, and MILP paths never substitute another solver on failure. RC-FW
reports a generalized Frank--Wolfe gap; Clarabel reports its primal/dual gap
and residuals; all methods report integer landing loss where available.

## Protocol v1 suite structure

- **Quality:** balanced, concentrated, asymmetric-depth, and buy-heavy books,
  with twelve independently seeded medium books per profile.
- **Scaling:** 300 to 30,000 declared retail orders, with multiple seeds at
  every size.
- **Budget:** the same seeded medium books at six MM-budget multipliers.
- **Decomposition:** monolithic and decomposed LP/quasi-Fisher on balanced and
  asymmetric books.
- **Reference:** small LP/conic/MILP comparisons with a fixed SCIP time limit;
  timeout incumbents remain timeouts.

Scenario generation adds configurable heavy-tailed sizes, side imbalance,
hot-market concentration, cross-market liquidity dispersion, depth levels,
group frequency, and MM coverage. These dimensions are fully recorded by the
checked-in protocol.
