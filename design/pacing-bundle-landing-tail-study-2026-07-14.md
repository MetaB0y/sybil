---
tags: [solver, benchmark, retained-cash, pacing, landing, welfare]
status: dated-reference
last_verified: 2026-07-14
---

# Pacing bundle and integer-landing tail study

> Follow-up (2026-07-17): the exact minimum-residual candidate policy described
> below was superseded by a one-microdollar support-equivalence band followed
> by retained-objective selection. The controlled comparison is recorded in
> `design/solver-experiments/objective-aware-landing-selection.md`; this file
> retains the original dated result.

## Decision

Keep [[Retained Cash Solver|`RetainedCashSolver`]] as the production default and
keep [[Pacing Bundle Solver|`PacingBundleSolver`]] as the leading research
candidate. The bundle is the stronger implementation of the continuous
retained-cash program in this development matrix, but its new lexicographic
landing adds a material 10,000-order latency tail and it has not been evaluated
on held-out or production-replay data.

The main correctness result is more important than the ranking: the previously
observed 67.9% retained-objective gap was a real projection bug. A pacing
certificate identifies an allocation, but the supporting LP may have a large
degenerate optimal face. Re-solving that LP and accepting an arbitrary optimal
basis can replace the certified allocation with a distant one. Landing now
selects, lexicographically, the supporting optimum nearest to the certified
continuous target. The primary matching-LP duals remain the published prices;
auxiliary distance-row duals are never treated as market prices.

On 110 verifier-valid rows of the complete 555-row development run, the bundle's maximum observed-best
retained-objective gap is 0.494%, versus 20.87% for RC-FW and 21.74% for the
five-step LP-SLP baseline. The bundle's maximum integer-landing loss is 0.534%
rather than 67.9%. This is strong diagnostic evidence that the repair addresses
the failure mode. One additional extreme-range row fails the price-support gate
explicitly and remains in the denominator. This is not confirmatory evidence
for the paper.

## Evidence boundary

The retained artifact is
`benchmarks/solver/results/2026-07-14-pacing-development-v2/`. It records source
revision `0b62dc1f`, 111 generated scenario groups, five methods, and all 555
declared outcomes. The analyzer found no missing rows, duplicates, unexpected
rows, or cross-solver scenario-fingerprint mismatches.

Seeds 16000--18403 were observed while the algorithms, landing, metrics, and
protocol were changing. They are development data. The matrix may motivate a
new preregistration, regression tests, and algorithm design; it must not be
relabeled held out. The scenarios are synthetic structural stresses, not a
calibrated model of production order flow. No suitable frozen replay corpus
currently exists.

The five methods are:

- a budget-blind LP negative control;
- the production-shaped five-step LP-SLP baseline;
- certified generalized Frank--Wolfe (RC-FW);
- the fully corrective pacing bundle; and
- corrected-epigraph Clarabel QuasiFisher.

No failed row is removed or replaced by another solver. The "observed-best"
quality reference is the best verifier-valid landed result among these methods
on a given book, not a proven optimum. The budget-blind LP is intentionally
infeasible when shared capital binds: only 35/111 rows pass verification, 76
violate MM budgets, and maximum capital use is 7.353 times budget. Its
conditional quality figures are therefore not a competitive result.

## The landing bug

### Symptom

The earlier bundle run lost about 68% of retained objective on
`flash-small-reference`, seed 16400, budget ratio 0.25. The continuous allocation
was already essentially integer and had the expected objective. Independent
rounding was not the source of the gap.

### Cause

The final pricing solve optimized the correct pacing-supported linear
objective, but that objective had multiple optima. HiGHS returned a different
vertex of the same supporting face. The vertex supplied valid-looking prices
and fills, yet was far from the certified atom mixture. Continuous convergence
therefore did not imply preservation through protocol landing.

This distinction matters generally:

```text
continuous certificate
    -> supporting objective and prices
    -> choose a primal point on a non-unique face
    -> integer quantities and hard-budget fixed point
    -> verifier-valid protocol result
```

Each arrow needs its own metric. A solver can have a tiny continuous gap and a
large landing gap, or can be integer-valid while its prices poorly support the
minting sector.

### Repair

Landing now performs two LPs:

1. solve the original pacing-supported matching objective and retain its market
   duals as clearing prices;
2. constrain the allocation to that primary optimal face and minimize L1
   distance to the certified target.

The exact face is attempted first on normally scaled books. HiGHS can report an
auxiliary optimum whose face row is materially infeasible on billion-unit
books, so those deliberately wide cases use a narrow `1e-8` relative near-face
band directly. The implementation checks exact face activity explicitly.

Before hard-budget checks, landing compares the already-available nearest-face,
primary-basis, and certified-target integer candidates under the primary prices.
It keeps the one with the smallest minting-duality residual and fails explicitly
if even the best exceeds $0.05. This is an economic support gate within the same
solver and price system, not a result from LP-SLP or another algorithm. The
allocation-movement and objective-loss diagnostics expose when it selects a
more conservative point.

The landed result then rounds, filters price-incompatible fills, and iterates
price-linearized MM budget rows to a fixed point. Exhausting eight budget steps
is a `PostProcessingFailure`; it is not silently repaired by changing solvers.
The diagnostics record retained-objective landing loss, L1 allocation movement,
and whether any final MM quantity had to be trimmed. No successful RC-FW or
bundle row in this matrix required such a trim.

### NO-side capital contract and deterministic LP execution

The 1,024-case conformance pass exposed a separate accounting bug after the
first retained run. `MmSide::capital_needed` was implemented as if every caller
provided the YES price, while the verifier, post-processing, and benchmark
reporter supplied the order's actual fill price. A NO-side order was therefore
complemented twice. The engine contract now uses actual traded-outcome prices:
either buy consumes its fill price and either sell consumes one minus its fill
price. LP code that starts from `p_yes` explicitly derives the order price.

The same minimized billion-unit case also made a degenerate HiGHS basis vary
across processes. HiGHS execution is now pinned to one thread, parallel mode
off, and random seed zero; MM objective indices are sorted before floating-point
accumulation. Five fresh-process repetitions of the minimized case and the full
1,024-case conformance pass succeeded. A subsequent fresh-process comparison
of the complete matrix produced identical non-timing solver outputs on all 555
rows. That comparison also exposed nondeterministic scenario hashes caused by
serializing `HashMap`s; benchmark fingerprints now canonicalize markets and MM
side maps. The retained artifact was regenerated after every correction rather
than relabeling old measurements. The present synthetic matrix mostly quotes
YES orders, so it is not strong empirical coverage of the NO-side fix. A future
frozen protocol should include paired YES/NO metamorphic books explicitly.

### Why a tiny primary tie-break is insufficient

A weighted sum such as "primary objective plus epsilon times distance" has no
portable epsilon across books whose coefficient and quantity ranges differ by
orders of magnitude. It can either fail to distinguish the degenerate face or
change the primary optimum and its prices. A prior utility-band experiment also
failed: its omitted shadow prices caused a generated buyer to fill at $1.00
despite a $0.901073144 limit. The two-stage lexicographic formulation states the
priority directly and preserves the primary market duals.

## Complete development result

All quality percentages below are gaps to the observed-best verifier-valid
landed result on the same generated book. Runtime and quality summaries
condition on successful rows, while the success count retains the declared
denominator.

| Metric | LP-SLP | RC-FW | Pacing bundle | Clarabel Quasi |
|---|---:|---:|---:|---:|
| Successful / declared | 111 / 111 | 110 / 111 | 110 / 111 | 109 / 111 |
| Termination | 87 converged, 24 capped | 82 converged, 28 capped, 1 landing failure | 105 converged, 5 capped, 1 support failure | 109 converged, 2 numerical failures |
| Median wall time | 15.19 ms | 74.88 ms | 74.75 ms | 30.85 ms |
| P95 wall time | 282.76 ms | 513.18 ms | 578.48 ms | 151.02 ms |
| P99 wall time | 434.86 ms | 1,423.79 ms | 2,185.52 ms | 311.87 ms |
| Maximum wall time | 473.01 ms | 2,244.11 ms | 2,313.45 ms | 338.37 ms |
| Welfare gap mean / P50 / P95 / max | 1.2750 / 0 / 0.0840 / 37.5938% | 2.8054 / 0.9588 / 6.6082 / 37.1955% | 1.8249 / 0.4693 / 6.0563 / 7.2036% | 2.1237 / 1.6237 / 6.0729 / 11.5312% |
| Retained gap mean / P50 / P95 / max | 1.0599 / 0.1395 / 2.5373 / 21.7429% | 0.5294 / 0.0002 / 0.0804 / 20.8714% | 0.00545 / 0 / 0.000615 / 0.4936% | 0.0379 / 0 / 0.4138 / 0.5357% |
| Landing loss P95 / max | -- | 0.0208 / 20.8685% | 0.00271 / 0.53439% | -- |
| Landing L1 P95 / max | -- | 0.3651 / 41.5304% | 0.08825 / 7.4923% | -- |
| Minting-duality residual P95 / max | $28.0197 / $62.7453 | $0.000000035 / $0.000000057 | $0.0000000106 / $0.0001598 | $0.00000145 / $0.000155 |

Welfare and retained objective answer different questions. The bundle optimizes
the paper's retained-cash objective, not risk-neutral net welfare. Its 7.20%
maximum welfare gap occurs on a small tight-budget book where its retained gap
is zero. That is an objective trade-off, not evidence that its retained solve
failed. Conversely, the LP-SLP baseline has a zero median welfare gap but loses
as much as 21.74% of retained objective and has large price-support residuals.

## Detailed tail findings

### Adversarial 80-order books

The largest welfare and retained gaps are concentrated in the four 80-order,
two-MM, 0.25-budget books. LP-SLP loses 19.77% retained objective on average and
up to 21.74%; RC-FW loses up to 20.87%. Three RC-FW landings move 38.49--41.53%
of total quantity after their cores hit the 100-update cap. The bundle has zero
gap on three rows and 0.494% on seed 17003 after its support gate rejects the
nearest candidate. Clarabel's maximum gap on these four rows is below 0.000001%.

This is the intended adversarial comparison against a short normal LP-SLP pass
and vanilla Frank--Wolfe: shared capital changes which LP face is active, and a
retained atom set can preserve and recombine earlier faces. It demonstrates a
mechanism on synthetic books. It does not prove that production markets have
this distribution or that only the bundle can solve it; Clarabel is the clear
counterexample to that stronger claim.

### Bundle landing tail after the repair

The bundle's worst landing row is `orders-00080`, seed 17003, budget ratio 0.25.
The nearest candidate was rejected by the minting-support gate; the supported
candidate moves 7.4923% of quantity and loses $2.8937, or 0.53439%, of retained
objective. The next tail is `mms-08`, seed 18301: 0.3052% movement and $9.4523,
or 0.05229%, of loss. Both minting-duality residuals are zero.

The earlier relaxed-only selector produced a smaller retained gap but a $21.14
minting-duality residual on `mms-04`, seed 18202. Among successful rows, the
economic support gate reduces the bundle maximum residual to $0.0001598. Thus the current implementation
chooses price support over a cosmetically perfect observed-best score. The
0.534% tail is now a measured integer-price recovery limitation, not a hidden
continuous convergence failure.

### Availability tails

RC-FW has one explicit landing failure on `mms-16`, seed 18403: its capped core
does not reach an MM-budget fixed point in eight projection steps. Clarabel has
two `InsufficientProgress` failures, on neutral seed 16000 at budget 4 and
wide-range seed 16201 at budget 10. Their large reported primal/dual residuals
are consistent with backend scaling or KKT progress failure, not a theorem that
the convex program lacks a solution.

The bundle also fails explicitly on wide-range seed 16203 at budget 0.25. Its
continuous certificate gap is only $0.000253, but none of the three integer
candidates is supported closely enough by the primary prices: the best
minting-duality discrepancy is $2,090.58 against $769,328.56 of
zero-temperature minting cost (0.272%). Accepting it would hide exactly the
price/allocation inconsistency this study was designed to expose. This row is
evidence that balanced integer recovery on ill-scaled degenerate faces remains
an unsolved part of the implementation, not that the pacing program failed to
converge.

### Latency tails

The lexicographic landing is not free. At 10,000 orders, bundle and RC-FW
medians are 1.786 s and 1.374 s, compared with 0.310 s for Clarabel and 0.315 s
for LP-SLP. Adding two L1 rows per order and occasionally rebuilding after an
exact-face numerical rejection dominates the tail. At 2,000 orders the bundle
median is 80.9 ms; with 16 MMs it is 222.5 ms.

This is now the main implementation target. Useful directions are a backend
with native lexicographic objectives, a reusable/copyable landing basis, or a
balanced integer recovery that requires less auxiliary LP structure. Any
optimization must retain the explicit landing and price-support diagnostics;
removing the second solve merely recreates the bug.

## Clarabel assessment

Clarabel is not fundamentally incompatible with market makers or flash
liquidity. On its 109 successful rows it is fast and its landed retained gap is
at most 0.536%. The two failures occur on slack/high-range cases with
`InsufficientProgress`, indicating numerical conditioning in the current
exponential-cone formulation or settings. They are backend failures, not
infeasibility certificates and not evidence against the Fisher formulation.

The next fair Clarabel study should predeclare scaling, equilibration, and
tolerance variants on development seeds, freeze one configuration, then run it
once on untouched seeds. Tuning only the two failed rows would be overfitting.

## Benchmark adequacy and next protocol

The current benchmark is substantially better as a structural development
suite: it separates order count from MM count, includes two-sided MM ladders,
binding-budget sweeps, concentrated books, wide numerical ranges, small
references, negative controls, failure denominators, certificate work, welfare,
retained objective, latency tails, landing movement, and price support.

It is not fully representative of real markets. A confirmatory empirical
section should contain three layers:

1. **Preregistered synthetic coverage.** Freeze code, tolerances, seeds,
   exclusions, primary metrics, and failure handling before generating an
   untouched corpus. Keep the current orthogonal and adversarial families.
2. **Calibrated simulation.** Match spread, depth, order-size, side imbalance,
   cancellation, MM coverage, and cross-market correlation distributions to a
   frozen descriptive dataset, without tuning outcomes by solver.
3. **Production replay.** Reconstruct complete batches, shared MM identities and
   budgets, and arrival-time boundaries from consented logs. Report coverage and
   every unsupported or malformed batch rather than silently filtering them.

The paper should make separate claims for objective quality, solver convergence,
integer landing, availability, and speed. It should report mean, P50, P95, P99,
and maximum where the tail matters, with paired per-book comparisons and every
cap/failure retained.

## Reproduction

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-pacing-development.json \
  --source-revision 0b62dc1f \
  --output-dir benchmarks/solver/results/2026-07-14-pacing-development-v2 \
  --overwrite

python3 scripts/benchmarks/analyze_solver_experiments.py \
  benchmarks/solver/results/2026-07-14-pacing-development-v2
```

The generated `summary.md`, `summary.json`, and `summary.csv` are the numerical
source of truth. This narrative rounds their values for readability.
