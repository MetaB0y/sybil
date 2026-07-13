# Solver experiments

This directory owns the reproducible solver-evaluation protocol. It is for
empirical claims, not for criterion-style microbenchmarks or a single ad-hoc
`matching-sim` comparison.

## Integrity rules

- `protocol-v1.json` fixes scenarios, solvers, seeds, budget points, iteration
  limits, time limits, primary metrics, exclusions, and interval construction
  before the full run.
- Every declared run produces one JSONL record. Panics, numerical failures,
  empty results, timeouts, verifier failures, and iteration caps are retained.
- All solvers in a run receive a byte-identical generated `Problem`; the
  analysis rejects fingerprint mismatches and duplicate/missing run keys.
- Welfare is the integer verifier-recomputed net welfare after the signed
  complete-set mint/burn adjustment. Floating backend objectives are not used
  for ranking.
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

First exercise every suite and solver path:

```bash
cargo run --release -p matching-sim --bin solver-experiments -- \
  --output-dir /tmp/solver-smoke --smoke --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/solver-smoke --allow-incomplete
```

For a publishable run, freeze the implementation in its own `jj` change and
pass that immutable commit:

```bash
cargo run --release -p matching-sim --bin solver-experiments -- \
  --source-revision <implementation-commit> \
  --output-dir benchmarks/solver/results/<date>-v1
python3 scripts/benchmarks/analyze_solver_experiments.py \
  benchmarks/solver/results/<date>-v1
```

The runner copies the protocol beside `results.jsonl` and writes machine and
toolchain metadata. The analyzer validates completeness, then creates
`summary.json`, `summary.csv`, `summary.md`, and deterministic SVG figures.

## Suite structure

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
