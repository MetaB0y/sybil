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

Protocol v1 is the retained historical evaluation. Protocol v2 is the current
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
