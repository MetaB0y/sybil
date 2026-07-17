# Structural price-sweep matching-oracle experiments

## Scope and acceptance policy

This ledger continues the exact fixed-pacing derivation in
[`price-pacing-dual.md`](price-pacing-dual.md), but changes the goal: replace
only the repeated HiGHS linear-oracle calls inside RC-FW and pacing bundle.
Final price discovery, supporting-face projection, price-linearized budget
repair, and integer landing still use HiGHS.

No single score decides these experiments. A candidate must first preserve:

1. explicit availability and verifier validity;
2. the continuous primal/dual certificate;
3. retained-cash and welfare quality, especially P95/max landing loss;
4. supporting-price and MM-budget integrity.

Among candidates that pass those gates, compare end-to-end P50/P95/P99/max
latency, oracle time, iteration count, active atoms, implementation complexity,
dependency exposure, and domain coverage. Development seeds can select or
reject engineering ideas but cannot support a held-out or production claim.

## Shared method

For fixed pacing coefficients, the matching LP has the price dual

```text
minimize    sum_i Q_i [c_i - payoff_i · p]+
subject to  p_yes,m + p_no,m = 1
            sum_(m in group g) p_yes,m <= 1
            p >= 0.
```

Every supported production order is one-hot on one binary market. An
independent market is therefore a one-dimensional convex hinge curve solved by
sorting its breakpoints. A categorical group is solved by merging all
nondecreasing marginal-slope segments and consuming at most one unit of
negative-slope capacity.

Primal recovery uses complementary slackness. Positive-surplus orders are full,
negative-surplus orders are empty, and only zero-surplus orders are free. Let
`d_m = D_yes,m - D_no,m`. For an independent market, its price is a
subgradient of `max(D_yes, D_no)`. For a categorical group, the YES-price
vector is a subgradient of `max(0, max_m d_m)`. Recovery is consequently an
interval problem: positive-price markets attain a common active difference,
zero-price markets do not exceed it, and the common difference is zero when
the price simplex has slack.

Every structural solve recomputes the primal objective and rejects the result
unless it agrees with the exact hinge-dual value to `1e-8` relative tolerance.

## Experiment SPO-001 — exact structural primal/dual oracle

- Date: 2026-07-17
- Status: core accepted; initial marginal selector rejected
- Hypothesis: breakpoint price minimization plus analytical subgradient
  recovery can replace a generic matching LP solve without changing the
  retained-cash algorithms or their certificates.
- Source: jj change `zmnxqkmp`.
- Unit workload: categorical fixture plus generated small, medium,
  market-like, and two-sided flash books. The tests make 123 fixed-coefficient
  structural-versus-HiGHS comparisons across development seeds `7400..7707`.
- Command:

  ```bash
  cargo test -p matching-solver --features retained-cash price_pacing_dual
  ```

All primal objectives and dual objectives matched HiGHS within the declared
relative tolerance.

The first end-to-end version initialized every marginal order empty and
adjusted them sequentially until the target difference was reached. It was
tested on all 244 rows in
`protocol-structural-oracle-development.json`, covering 61 cases per solver:
market-like books, a six-point tight-budget flash sweep, numerical-range
stress, and fixed-order-count 1/4/16-MM scaling. Seeds `21000..21502` are all
development-only.

| Solver | Success | P50 / P95 wall | Oracle P50 | Retained gap mean / max |
|---|---:|---:|---:|---:|
| RC-FW + HiGHS | 60/61 | 60.17 / 574.16 ms | 29.89 ms | 0.0042% / 0.0844% |
| RC-FW + structural | 60/61 | 30.20 / 339.25 ms | 1.11 ms | 0.0042% / 0.0844% |
| Bundle + HiGHS | 60/61 | 47.99 / 578.11 ms | 13.48 ms | 0.0063% / 0.3795% |
| Bundle + structural | 61/61 | 58.98 / 466.72 ms | 0.85 ms | 0.0073% / 0.3795% |

The aggregate looked promising but failed the quality gate. Numerical seed
`21204` at budget `0.25x` had an essentially exact continuous bundle optimum
yet lost `$251.029877` (`0.066311%`) during landing and moved `0.398513%` of
allocation mass. The sequential selector chose an extreme point on a
degenerate marginal face. Decision: keep the exact structural core and seed
`21204`; reject the marginal selector.

## Experiment SPO-002 — categorical active-difference endpoints

- Date: 2026-07-17
- Status: rejected
- Hypothesis: choosing either the lowest or highest feasible common
  `d_m` for active categorical markets will produce a better marginal face.
- Workload: numerical seed `21204`, budget `0.25x`, all four oracle/algorithm
  combinations.
- Result: lower and upper endpoints produced the same `$251.029877` bundle
  landing loss and `0.398513%` movement.
- Decision: reject both endpoints. The harmful degeneracy is within each
  market's marginal orders, not the choice of group-level active difference.

## Experiment SPO-003 — previous-solution warm start with sequential repair

- Date: 2026-07-17
- Status: rejected
- Hypothesis: retaining the preceding oracle allocation and choosing the
  feasible group difference nearest to it will emulate simplex basis warmth
  and stop face jumps as pacing coefficients move.
- Workload: the SPO-002 counterexample.
- Result: the same `$251.029877` loss remained. The group target was stable,
  but sequential repair still concentrated the necessary change into the
  earliest marginal orders.
- Decision: keep the previous-allocation state and nearest group target as
  useful continuity, but reject sequential repair.

## Experiment SPO-004 — balanced marginal-face recovery

- Date: 2026-07-17
- Status: retained as an experimental backend
- Hypothesis: distribute each required difference correction proportionally
  over all marginal orders' available capacity, starting from the previous
  optimum. This selects an interior, temporally stable point instead of an
  arbitrary cap-extreme point.
- Source: jj change `zmnxqkmp`.
- Protocol:
  `benchmarks/solver/protocol-structural-oracle-development.json`.
- Commands:

  ```bash
  cargo run --release -p matching-sim --all-features \
    --bin solver-experiments -- \
    --protocol benchmarks/solver/protocol-structural-oracle-development.json \
    --source-revision zmnxqkmp-balanced-face-v2 \
    --output-dir /tmp/structural-oracle-balanced-v2 --overwrite
  python3 scripts/benchmarks/analyze_solver_experiments.py \
    /tmp/structural-oracle-balanced-v2
  ```

The counterexample's structural bundle landing loss fell from `$251.029877`
to `$0.000158`. The complete run retained all 244/244 declared records with no
fingerprint mismatches, duplicates, or verifier-invalid candidates.

| Solver | Success | P50 / P95 / max wall | Oracle P50 | Landing loss P95 / max |
|---|---:|---:|---:|---:|
| RC-FW + HiGHS | 60/61 | 51.36 / 372.46 / 454.88 ms | 25.67 ms | $0.134841 / $1.020827 |
| RC-FW + structural | 60/61 | 21.28 / 270.62 / 434.53 ms | 0.78 ms | $0.134841 / $1.020827 |
| Bundle + HiGHS | 60/61 | 42.02 / 542.61 / 598.50 ms | 11.33 ms | $0.094741 / $3.990183 |
| Bundle + structural | 60/61 | 31.53 / 431.75 / 471.78 ms | 0.57 ms | $0.094741 / $3.990183 |

Landed quality aggregates were effectively equal: bundle retained gaps were
`0.0063%` mean and `0.3795%` max for both backends; RC-FW was `0.0038%` mean
for HiGHS and `0.0039%` for structural, with the same `0.0844%` max. The
structural bundle matched HiGHS within `1e-12` relative objective on 58/60
jointly successful cases. RC-FW did so on 53/60; its largest continuous-iterate
difference was `0.00764%` on numerical seed `21200` at `0.25x`, where both
landed successfully. This is expected sensitivity of a capped Frank--Wolfe
path to a different optimum on a degenerate linear-oracle face, not a
certificate violation.

Remaining failures were shared by backend:

- bundle numerical seed `21200`, budget `0.25x`: no integer point passed the
  supporting-price residual gate;
- RC-FW 16-MM seed `21501`, budget `0.25x`: integer landing did not reach a
  budget fixed point in eight steps.

Decision: retain `LinearOracleBackend::StructuralPriceSweep` for experiments
and differential testing. Keep `Highs` as the production default. This is a
real example of a feasible domain-specific solver library: the repeated
zero-RHS matching oracle becomes a small sorting/interval algorithm, avoids
Clarabel entirely, and is much simpler than writing a general LP or conic
solver. It intentionally does not handle arbitrary payoff vectors,
price-linearized budget rows, supporting-face projection, or integer landing.

Before considering a default change:

1. freeze and run untouched seeds;
2. add replay books captured at the sequencer boundary rather than claiming
   the synthetic “market-like” profile is calibrated flow;
3. test larger order counts and adversarial tie density;
4. isolate benchmark process noise and compare multiple machines;
5. either keep HiGHS as a landing-only dependency with an explicit operational
   rationale or replace that narrower LP path separately.

## Experiment SPO-005 — zero-tolerance, wide-scale conformance

- Date: 2026-07-17
- Status: accepted regression
- Trigger: the new structural RC-FW conformance test minimized a one-market
  book containing both billion-unit quantities and a near-zero-limit marginal
  order.
- Result before repair: two structural oracle calls reached the exact optimum,
  but subtracting independently accumulated scores near `1.6e13` nanos left a
  `0.001953125`-nano positive gap. With the conformance test's deliberately
  zero absolute and relative tolerances, RC-FW attempted another line search,
  found a zero step, and mislabeled the verifier-ready result
  `NumericalFailure`.
- Decision: preserve proptest regression
  `9f52d6f39599cbcaec6aa89a718bcb627b6fc350ff30da0ae6782fdfe31da7f6`.
  Accept convergence when the certified gap is below a 32-ULP,
  score-scale-aware subtraction floor, while continuing to report the
  unsnapped gap. This is a representation bound, not a relaxed economic
  tolerance.
