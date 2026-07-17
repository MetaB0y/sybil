# Direct price–pacing dual experiments

Tracking issue: [#173](https://github.com/MetaB0y/sybil/issues/173)

## Shared derivation

For fixed MM pacing factors `alpha`, the ordinary zero-temperature matching
LP has the price dual

```text
minimize    sum_i Q_i [c_i(alpha) - payoff_i · p]_+
subject to  p_yes,m + p_no,m = 1
            sum_{m in categorical group g} p_yes,m <= 1
            p >= 0.
```

The implementation carries the existing `SHARE_SCALE` and nanos factors; the
display suppresses them. Applying
`psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha)` gives the joint convex
dual

```text
minimize_{p, alpha} fixed_price_hinges(p, alpha)
                          - sum_k B_k ln(alpha_k).
```

Production orders touch one binary market. At fixed `alpha`, each market is
therefore a one-dimensional convex piecewise-linear hinge curve. A standalone
market is solved by its breakpoints. A categorical group is a separable
resource-allocation problem: merge the markets' nondecreasing marginal-slope
segments and consume at most one unit of negative-slope capacity.

This differs from the 2026 Taubman–Gleyzer scalar reduction, which describes a
different parimutuel mechanism without liquidity providers. It is also
different from `PacingBundleSolver`: that method optimizes only `alpha` and
still calls the primal HiGHS matching oracle.

## Experiment PPD-001 — exact fixed-pacing price dual

- Date: 2026-07-17
- Status: accepted as a building block
- Hypothesis: the structural breakpoint solver returns the same optimal value
  as `ReusableLpOracle` for arbitrary fixed pacing coefficients, including
  categorical minting groups and MM buy/sell reductions.
- Source: jj change `osnwvoox`.
- Development data: deterministic fixtures and generated seeds `7400..7408`;
  no held-out seed at or above `50000`.
- Command: `cargo test -p matching-solver --features lp price_pacing_dual`
- Acceptance: direct and HiGHS objectives agree to `1e-8` relative tolerance
  on every declared case.
- Result: passed on the categorical minting fixture and all eight shaded
  generated books.
- Decision: retain. This validates the fixed-pacing price-dual algebra and
  exact segment merge, not a complete clearing solver.

## Experiment PPD-002 — exact alternating price/pacing coordinates

- Date: 2026-07-17
- Status: rejected
- Hypothesis: alternating the exact fixed-`alpha` price minimizer with exact
  per-MM `alpha` minimizers reaches the global joint-dual optimum.
- Source: jj change `osnwvoox`.
- Development data: 100 generated small books, seeds `7600..7699`; no held-out
  seed at or above `50000`.
- Command:
  `cargo test --release -p matching-solver --features lp scan_coordinate_dual_tightness_and_timing -- --ignored --nocapture`
- Comparator: RC-FW's continuous objective plus its certified gap. A direct
  dual value above that certified upper bound is a coordinate-stall witness.
- Result: 64/100 books exceeded the comparator by more than `1e-8` relative.
  Worst: seed `7654`, `2.940403%` relative excess. The method reported
  coordinate stationarity after two sweeps on that case.
- Timing diagnostic: direct bound evaluation took `0.021684s` aggregate;
  RC-FW took `1.828323s`. These are intentionally **not** called a speedup:
  the direct route produced only a bound, while RC-FW also recovered and
  landed fills.
- Decision: reject plain cyclic/exact block-coordinate minimization.
  Nonsmooth hinges admit coordinatewise stationary points that are not global
  optima. Retain seed `7654` as a permanent ledger counterexample.
- Revisit only if coordinates are coupled by a globally convergent mechanism
  (for example proximal bundle, smoothing continuation with exact-bound
  evaluation, or a semismooth active-set solve).

## Experiment PPD-003 — smoothed first- and second-order continuation

- Date: 2026-07-17
- Status: rejected
- Shared rule: softplus smoothing was used only to search; every reported
  bound reevaluated the original nonsmooth hinge objective.
- Data/comparator: the same seeds `7600..7699` and RC-FW certified upper
  bounds as PPD-002.

### PPD-003a — moderate projected-gradient continuation

- Schedule: one-cent initial temperature to `1e-6` dollars.
- Result: worst relative excess `0.026706%`; 86/100 exceeded `1e-8`.
  All stages reported convergence. Aggregate direct-bound time `0.517407s`
  versus RC-FW end-to-end `1.901457s` (not an end-to-end speedup).
- Decision: reject; finite-temperature/search error remained material.

### PPD-003b — cold projected-gradient continuation

- Schedule: one-cent initial temperature to `1e-8` dollars, up to 800
  iterations per stage, no objective-stagnation shortcut.
- Result: worst relative excess `0.000398447%`; 33/100 exceeded `1e-8`;
  46/100 hit a stage cap. Aggregate direct-bound time `9.407141s` versus
  RC-FW end-to-end `1.979010s`.
- Decision: reject; accurate but slower even before primal recovery.

### PPD-003c — damped projected Newton

- Variant: exact softplus Hessian (rank-one order terms plus log-utility
  curvature), dense Cholesky in `markets + MMs` dimensions, projected
  categorical-simplex steps.
- Result: 99/100 hit a stage cap; worst excess regressed to `0.495357%`.
  Aggregate direct-bound time `21.241369s` versus RC-FW `1.877052s`.
- Decision: reject and remove the implementation. Projection does not preserve
  the active-set geometry needed for a useful Newton direction.

The next route should solve the exact epigraph/cone dual. Its hinge-row dual
multipliers recover fills directly, avoiding both smoothing and a separate
marginal-order recovery heuristic.

## Experiment PPD-004 — exact price-side exponential cone

- Date: 2026-07-17
- Status: retained as a research reference; rejected as a production candidate
- Hypothesis: solving the joint hinge/log dual as one exponential-cone program
  will retain the direct formulation's tight certificate, recover fills from
  hinge-row dual multipliers, and improve end-to-end availability or latency
  over the existing retained-cash solvers.
- Source: jj change `osnwvoox`.
- Development protocol:
  `benchmarks/solver/protocol-price-pacing-development.json`.
  It declares 59 cases per solver and 236 total rows across market-like,
  tight two-sided flash, numerical-range, and 1/4/16-MM workloads. Seeds
  `19100..19602` are development-only and below the held-out boundary `50000`.
- Commands:

  ```bash
  cargo run --release -p matching-sim --all-features \
    --bin solver-experiments -- \
    --protocol benchmarks/solver/protocol-price-pacing-development.json \
    --source-revision osnwvoox-development \
    --output-dir /tmp/price-pacing-development-v1 --overwrite
  python3 scripts/benchmarks/analyze_solver_experiments.py \
    /tmp/price-pacing-development-v1
  ```

The exact cone core worked. Its successful continuous solutions had a median
certified relative gap of `0.000002%`, and the exact projected price/pacing
dual remained a valid upper bound. Every returned integer candidate passed the
verifier. End-to-end results did not support the performance hypothesis:

| Solver | Success | Median runtime | Retained gap mean / P95 / max |
|---|---:|---:|---:|
| Fill-side Clarabel Quasi | 56/59 | 33.4 ms | 0.0129% / 0.0576% / 0.4271% |
| Direct price-side cone | 53/59 | 167.9 ms | 0.0172% / 0.0640% / 0.4808% |
| Pacing bundle | 58/59 | 85.1 ms | 0.0001% / 0.0000% / 0.0055% |
| RC-FW | 58/59 | 131.3 ms | 0.0059% / 0.0325% / 0.0918% |

The direct solver's six failures were:

- flash seeds `19200` at `0.5x` and `19201` at `0.1x`: no integer candidate
  met the `$0.05` supporting-price residual gate;
- numerical seed `19303` at `0.25x`: supporting-price/nearest-face recovery
  produced no usable rows;
- all three one-MM scaling seeds `19400..19402` at `0.25x`: Clarabel
  terminated with `InsufficientProgress`.

The market-like slice was 15/15 and landed at effectively zero retained loss,
but its median runtime was `741.4 ms`, versus `177.0 ms` for RC-FW,
`176.9 ms` for pacing bundle, and `79.0 ms` for fill-side Clarabel. The tail
diagnosis was more important than the aggregate ranking: numerical seed
`19302` at `0.25x` had only `$0.016182` continuous core gap, yet integer
landing lost `$2043.433470` (`0.480789%`) and moved `1.448394%` of allocation
mass. The direct formulation found the continuous face; the arbitrary
hinge-dual point on that degenerate face was a poor integer target.

Decision:

- retain the fixed-pacing price oracle cross-check as test evidence and the
  exact cone implementation as an independent, feature-gated
  certificate/reference;
- do not make it the production solver or spend more effort tuning generic
  cone tolerances;
- remove the rejected smoothing/Newton implementations while retaining
  PPD-002/003 here;
- make the next experiment a marginal-face recovery method: strict-surplus
  orders are fixed full/zero by the direct dual, while only zero-surplus orders
  enter a small integer-friendly active-set LP;
- if that recovery does not remove the availability and landing tails, use the
  direct dual only as a shadow certificate and advance the pacing bundle to a
  frozen held-out comparison.

## Experiment PPD-005 — marginal-face integer recovery

- Date: 2026-07-17
- Status: conditional selector retained for the research reference; direct
  route still rejected as a production candidate
- Hypothesis: Clarabel's hinge-row multipliers identify only one point on a
  degenerate optimal face. Classifying orders by direct-dual surplus and
  reopening all zero-surplus caps lets the supporting LP select a more
  integer-friendly point.
- Source: jj change `zsunwsvk`.
- Data: the PPD-004 24-row smoke and complete 236-row development protocol;
  no held-out seeds.

### PPD-005a — unconditional expanded KKT face

For projected direct prices and pacing factors, orders with surplus above
`$0.000001` were strict-full, those within the tolerance were marginal, and
strict-negative or zero-budget-MM orders remained capped at zero. The
supporting LP received full caps for strict-full and marginal orders rather
than Clarabel's recovered quantities.

Decision: reject blind expansion. On the six-case smoke, direct availability
fell from 5/6 to 4/6, a formerly successful market-like book failed its
supporting-price gate by `$3.27`, and retained landing loss worsened on
numerical and multi-MM cases. The larger face contained a better flash
candidate, but it also exposed arbitrary worse bases.

### PPD-005b — conditional two-face selection

The retained variant:

1. lands the original restricted hinge-dual target first;
2. retries the expanded KKT face only when Clarabel reports exact `Solved` and
   restricted landing either fails or loses more than one basis point of the
   continuous objective;
3. compares verifier-ready candidates by the actual retained-cash objective
   and never replaces a successful restricted result with a worse expanded
   result;
4. does not retry `AlmostSolved` cones, where development showed extra work
   without a meaningful quality win.

Commands:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-price-pacing-development.json \
  --source-revision zsunwsvk-hybrid-face-final \
  --output-dir /tmp/price-pacing-hybrid-face-final-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/price-pacing-hybrid-face-final-development
```

Compared with PPD-004:

| Direct price-side cone metric | PPD-004 | PPD-005b |
|---|---:|---:|
| Successful cases | 53/59 | 55/59 |
| Median runtime | 167.9 ms | 219.4 ms |
| Retained gap mean / P95 / max | 0.0172% / 0.0640% / 0.4808% | 0.0140% / 0.0427% / 0.4259% |
| Landing loss P95 / max | $4.497885 / $2043.433470 | $4.517481 / $35.068550 |
| Relative landing loss P95 / max | 0.063974% / 0.480789% | 0.042945% / 0.425929% |
| Allocation L1 P95 / max | 0.583757% / 1.448394% | 0.753394% / 2.900202% |

Five of 55 successful cases selected the expanded face; 50 retained the
original target. Numerical seed `19302` at `0.25x` reduced landing loss from
`$2043.433470` to `$26.128686`, and flash seed `19200` at `0.1x` reduced it
from `$0.545805` to zero. Expanded recovery also turned numerical seed `19303`
and flash seed `19201` at `0.1x` from explicit failures into verifier-valid
candidates, although the latter retained a `0.425929%` loss.

The four remaining failures are flash seed `19200` at `0.5x` at the
supporting-price gate and all three one-MM scaling seeds `19400..19402` at
Clarabel `InsufficientProgress`.

Decision: retain the conditional selector because it strictly guards candidate
quality and removes the catastrophic landing-dollar tail. It does not justify
a held-out production comparison: availability remains below pacing bundle,
latency is higher than RC-FW, the worst relative tail remains material, and
allocation movement worsened. Conclude the direct cone line as a
certificate/reference and move production-candidate evaluation to pacing
bundle.

## Sequence status

1. Exact fixed-pacing value oracle: **accepted** (PPD-001).
2. Exact cyclic price/`alpha` minimization: **rejected** (PPD-002).
3. Smoothed first/second-order search: **rejected and removed** (PPD-003).
4. Exact price-side cone with primal recovery: **reference only** (PPD-004).
5. Blind marginal-face recovery: **rejected** (PPD-005a).
6. Conditional two-face recovery: **reference improvement only** (PPD-005b).
7. Direct cone as production candidate: **stopped**; freeze pacing bundle for
   the next held-out comparison.
