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

## Experiment ECD-003 — frozen production-promotion evaluation

### Protocol

`benchmarks/solver/protocol-bundle-promotion-v1.json` and the unchanged
candidate implementation were frozen and pushed at
`c9fb939d521f92fd23e099208f15c931ddb51352` before any evaluation seed was
run. The declared `70000..71002` range was then evaluated exactly once. The
checked-in artifact at
`benchmarks/solver/results/2026-07-17-bundle-promotion-v1/` is complete:
136/136 rows, no duplicates, and no cross-solver fingerprint mismatch.

### Result

Both solvers returned verifier-valid allocations on all 68 books. Pairing by
book and budget, the exact bundle had a higher landed retained-cash objective
in 49 cases and tied RC-FW in 19; it was lower in none.

| Metric | RC-FW | Exact bundle |
|---|---:|---:|
| Verifier-valid availability | 68/68 | 68/68 |
| Iteration caps | 22 | 2 |
| P50 / P95 / P99 / max | 191 / 462 / 2,317 / 2,334 ms | 176 / 425 / 477 / 478 ms |
| Retained gap P50 / P95 / max | 0.0004 / 0.0818 / 17.5966% | 0 / 0 / 0% |
| Certificate relative gap P95 | 0.141580% | 0.00000035% |
| Landing loss P95 / max | `$0.620` / `$82.993` | `$0.00000003` / `$0.646` |
| Allocation L1 P95 / max | 0.422 / 38.542% | 0.012 / 0.281% |
| Oracle calls P50 / P95 | 22 / 101 | 14 / 101 |

The most important held-out counterexample was the 80-order balanced
fragmented slice. RC-FW hit its iteration cap in two of three cases and lost
16.5--17.6% of the retained objective during integer landing; the exact bundle
converged in all three, had zero measured retained gap, and ran about four
times faster. At 10,000 orders its median runtime was `412 ms` versus
`2,309 ms`. At 16 independent MM components it was `14 ms` versus `158 ms`.

The candidate is structurally more involved than a single generalized
Frank--Wolfe loop: it adds an exact connectivity router and a fully corrective
restricted master. That source complexity is a real cost, not hidden by the
runtime result. Operationally, however, the landed path is simpler: it reached
the iteration cap ten times less often, retained a median of three and at most
30 active atoms, and eliminated the quality failure tail that otherwise
requires diagnosing continuous convergence and lossy landing together.

### Decision

The frozen promotion rule passes. Promote the exact-component pacing bundle
behind one named production-solver facade, retain RC-FW as an independent
certified reference and fallback implementation, and keep experimental solver
composition out of sequencer call sites. Move shared assembly/landing support
to objective-oriented internal modules while doing so; production should not
depend conceptually on the approximate `DecomposedSolver`.

Do not retune or rerun this protocol. Future changes need new seeds and a new
versioned promotion protocol. The checked-in rows remain the immutable
baseline for this decision.

### Production follow-through

The promoted composition is exposed as `ProductionSolver`; sequencer creation
and restore use that facade rather than spelling
`ExactComponentSolver<PacingBundleSolver>` at call sites. The pacing bundle,
exact router, and their private HiGHS machinery now belong to the
`retained-cash` feature. The broader `lp` feature adds only the public
risk-neutral LP baseline and approximate coordinated decomposition.

The first full sequencer test exposed one consensus-relevant integration bug:
component IDs inherited randomized `MarketSet` hash iteration, and component
results were concatenated in that incidental order. Identical independently
constructed books could therefore emit the same economic fills in a different
sequence, changing account event digests and state roots. The implementation
now sorts markets before assigning component IDs and sorts merged fills by
admitted order ID. The minimized `state_root_determinism` property and the
complete 11-test sequencer invariant target pass after the repair.

Shared result aggregation also moved out of `decomposed.rs` into the neutral
`component_assembly.rs`. Exact production decomposition no longer depends
conceptually or structurally on the approximate proportional-response solver.

## Experiment ECD-004 — adversarial one-component audit

### Question

Does the promoted solver remain operationally credible when an adversary
removes every decomposition opportunity? If so, is the exact router cheap
enough on that hostile topology to justify keeping it as an opportunistic
accelerator?

The answer is deliberately split in two. `PacingBundleSolver` is the economic
algorithm and security baseline. `ExactComponentSolver` is only a routing
optimization; success on fragmented books cannot compensate for failure of
the monolith on a connected book.

### Frozen threat model and protocol

`benchmarks/solver/protocol-adversarial-connectivity-v1.json` declares 20
scored book/budget cases and 60 solver rows before evaluation:

- 64 markets with either 10,000 or 50,000 accumulated retail orders;
- one broad maker connecting every market through 384 orders, which fits one
  production submission; or
- sixteen local 24-order MM constraints plus one economically active,
  one-share-per-market 64-order bridge.

The bridge uses maximally willing YES bids and retains a small positive
generated budget under both pressure ratios. The local-MM experiments also use
generated rather than per-constraint LP-calibrated budgets: the development
smoke showed that independent unconstrained calibration legitimately assigns
zero to inactive local makers, which turns the intended connectivity test into
a zero-budget post-processing test. The global-MM experiments retain
LP-limit-value calibration. Every MM constraint is bounded by the API's
512-order submission limit. All cases must have exactly one economic
component.

The paired solvers are RC-FW, monolithic pacing bundle, and the exact-component
wrapper around the same bundle. Solver order rotates within each book. The
untouched evaluation ranges are `72000..72002`, `72100..72101`,
`72200..72202`, and `72300..72301`; smoke mode maps these to disjoint
42,000-series development seeds.

### Preregistered decision rule

The complete matrix, fingerprints, verifier checks, and one-component topology
are hard gates. The monolithic bundle may not lose landed retained-cash
objective to RC-FW by more than
`max(1,000 nanodollars, 1e-8 × |RC-FW objective|)`. Its maximum wall time must
remain below three seconds at 10,000 retail orders and the deployed ten-second
block interval at 50,000.

On every connected pair, the wrapper and monolith must have identical
termination, landed allocation, welfare, and retained objective. Retain the
router in `ProductionSolver` only if those gates pass and its paired P95 scan
overhead at each scale is at most the larger of 5% and 50 milliseconds.
Otherwise make the monolithic pacing bundle the production default.

This section, the attack generator, and the protocol must be pushed before any
72,000-series seed is generated. Record the immutable freeze revision and the
single evaluation below after the run; do not retune this protocol.

### Frozen evaluation result

The attack generator, protocol, and decision rule were frozen and pushed to
`origin/main` at
`f82e2455c1c355e2d09bf25ab86323b10c5d7c66` before any evaluation seed was
generated. The one full run is retained at
`benchmarks/solver/results/2026-07-17-adversarial-connectivity-v1/`.

The artifact is complete: 60/60 declared rows, 20/20 book/budget groups, no
duplicates, no cross-solver fingerprint mismatch, and exactly one component in
every row. The monolithic and wrapped bundles both converged and passed the
verifier on all 20 cases. RC-FW produced 19 benchmark-successful rows, with two
iteration caps and one explicit numerical failure.

The monolithic bundle improved landed retained-cash objective over RC-FW in
20/20 pairs, tied none, lost none, and had no material regression under the
frozen tolerance. The smallest improvement was 14,593 nanodollars and the
largest was 4,023,940,614 nanodollars. Thus the core algorithm's quality and
availability gates pass independently of decomposition.

The wrapper and monolith had identical termination, retained objective, net
and gross welfare, fill count, total filled quantity, landing diagnostics, and
per-MM utilization in every pair. The artifact does not persist a full fill
vector hash, but the connected branch in `ExactComponentSolver::solve`
directly returns `self.inner.solve(problem)` without assembly, so no
allocation-transforming wrapper path runs. Their timing was:

| Declared retail orders | Monolith P50 / max | Wrapper P50 / max | Paired wrapper-overhead P95 | Ratio P95 |
|---:|---:|---:|---:|---:|
| 10,000 | 2.034 / 3.522s | 2.024 / 3.557s | 53.3ms | 1.0281× |
| 50,000 | 82.279 / 85.968s | 82.669 / 85.594s | 394.6ms | 1.0048× |

The router-overhead gate passes: the allowed P95 bounds were 175.8ms and
4,289.7ms respectively. The security-baseline latency gate fails at both
scales. The 10,000-order maximum exceeded its three-second target by 17%; the
50,000-order maximum exceeded the deployed ten-second block interval by 8.6×.
The cheap 64-order bridge was sufficient to force the same whole-book path;
the failure is therefore in core connected-book scaling, not graph analysis.

### Decision and production follow-through

Apply the preregistered rule: make `ProductionSolver` a named facade over
monolithic `PacingBundleSolver`. Retain `ExactComponentSolver<S>` as an
explicit opt-in exact accelerator and as benchmark topology instrumentation,
but do not treat it as part of the production security architecture.

This is a complexity and threat-model decision, not a latency fix. The
monolithic bundle remains economically stronger and more available than RC-FW,
but a 50,000-order connected batch is not safe for the current ten-second
cadence. The next production-capacity work must target the monolithic oracle /
landing path or enforce a measured resource boundary; fragmented-book
speedups cannot satisfy that requirement.

Do not rerun or retune ECD-004. Any proposed connected-book solver improvement
needs a new versioned protocol and untouched seeds.
