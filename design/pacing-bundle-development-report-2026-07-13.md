---
tags: [solver, benchmark, retained-cash, pacing, landing]
status: dated-reference
last_verified: 2026-07-13
---

# Fully corrective pacing bundle: development report

## Decision

Keep `RetainedCashSolver` as the production default for now. Add
`PacingBundleSolver` as the stronger research candidate for the same convex
retained-cash objective, and use the checked-in development protocol to design
a future frozen evaluation.

The candidate is both conceptually cleaner and empirically promising. It turns
the retained-cash variational identity into a fully corrective bundle over
ordinary matching-LP optima. On the complete 444-row, four-solver development
matrix it used fewer LP-oracle calls than RC-FW, cut RC-FW's median and tail
latency, and eliminated every continuous iteration cap. It is not yet a
production replacement: degenerate supporting LP faces can create a material
continuous-to-integer landing loss, and the restricted master becomes
expensive as the number of market makers grows.

The corrected-epigraph Clarabel QuasiFisher solve is the strongest landed-
quality reference in this matrix. It was competitive with or faster than the
bundle on most axes and usually landed at the best observed retained objective,
but failed numerically on 2 of 111 declarations. That result is evidence
against declaring any one implementation the winner: the bundle currently has
the best availability and conservative certificate behavior, while Clarabel
has the best quality/latency combination conditional on success.

## Evidence boundary

This report is development evidence. `protocol-pacing-development.json` uses
seeds 16000--18403; the implementation, landing, and diagnostics were changed
after observing these rows. Every declared row and every failed outcome is
retained, but the matrix is not held out and cannot confirm a paper claim.

The protocol separates two axes that older scaling runs confounded:

- 80, 400, 2,000, and 10,000 orders at exactly two MMs;
- 1, 2, 4, 8, and 16 MMs at exactly 2,000 orders;
- a six-point two-sided flash-budget sweep;
- neutral, concentrated, heavy-tailed numerical, and tiny reference books.

Every instance was run with the five-step LP-SLP baseline, RC-FW, the pacing
bundle, and corrected-epigraph Clarabel QuasiFisher. No row was removed, no
failed run was replaced by another solver, and the observed-best landed
objective is reported only as a comparison—not as an optimality proof.

## Algorithm

For each MM, shifted retained-cash utility satisfies

```text
psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha).
```

At fixed pacing vector `alpha`, the inner maximization is the exchange's
ordinary matching LP. Its optimum is simultaneously a cutting plane of the
convex pacing dual and a feasible atom of the primal matching polytope. The
solver retains distinct atoms, represents its current allocation as a convex
mixture, and fully corrects that mixture with pairwise exact concave line
searches. A final oracle call gives a global upper bound; the atom mixture gives
the primal lower bound.

This is best described as a fully corrective pacing bundle or simplicial
decomposition method. It is not BFGS, a projected subgradient heuristic, a
fixed-point method, or an ordinary LP with one budget linearization. It keeps
the reason to use the theoretical formulation: every converged core result has
an explicit continuous retained-cash certificate.

The practical implementation remains modest:

1. one shared retained-cash objective model;
2. one reusable, warm-started HiGHS matching oracle;
3. a sparse list of LP atoms and convex weights;
4. pairwise scalar line searches in the restricted master; and
5. the shared price/integer projection.

## Correctness repairs found by building it

The bundle experiment exposed several defects that would otherwise have made
both algorithms and their benchmark look better than they were.

### Minting is an epigraph, not equality balance

The zero-temperature paper objective uses
`C_0(D) = max_omega D_omega`, represented by `M >= D_omega`. The LP core used
equality rows. Equality accidentally required outcome demands to balance before
the minting sector acted and disagreed with the paper on one-sided demand. The
LP rows are now upper epigraph inequalities, the direct objective evaluator
uses the corresponding max formula, and a regression test proves that a lone
60-cent YES bid cannot obtain newly minted supply for free.

### The oracle certificate needs a dual upper bound

The returned HiGHS primal objective was previously treated as the exact linear
oracle optimum. That is not a conservative certificate under floating-point
termination tolerances. The reusable oracle now gives every column a finite
analytical bound and combines row duals, reduced costs, and those bounds into a
Lagrangian upper bound. RC-FW and the bundle both use it. Tests require the
upper bound to dominate the returned primal and remain tight on representative
books.

### Landed objective must use landed settlement cash

The benchmark evaluated landed fills with the continuous zero-temperature cost
even after individual fills had been trimmed at fixed prices. That can make a
post-processed result appear to beat its own continuous upper certificate. The
landed retained objective now uses the protocol's signed mint/burn cash derived
from actual fill prices. The benchmark separately records
`|C_0(D) - p·D|` as a supply-optimality / minting-duality diagnostic.

### Blind trimming after price discovery is unsafe

Reducing fills after prices are extracted can leave the modeled equilibrium
face. It may still satisfy simple order limits and hard budgets while changing
minting economics and the retained objective. Retained-cash projection now
checks the rounded allocation at its discovered prices, adds price-linearized
budget rows, and resolves. It finalizes only at a fixed point; eight exhausted
projection steps produce an explicit `PostProcessingFailure`.

### Degenerate LP faces expose the remaining landing problem

A pacing-supported LP can have many optimal primal points. The final basis can
therefore choose another supporting optimum rather than the certified atom
mixture. On the final matrix this lost 0.1810% of retained objective at the
tightest flash budget. Bundle landing loss was small at the median (`$0.0049`)
but reached `$9.42` at P95 and `$3,298.50` at the maximum.

Two-sided utility bands around the target MM values appeared to solve this
quality problem, but conformance testing found that their shadow prices distort
the extracted market duals: a generated buyer was filled at `$1.00` above its
`$0.901073144` limit. Those rows were removed. The principled follow-up is a
separate supporting-price feasibility problem on the original optimal dual
face, not auxiliary primal constraints whose multipliers are omitted from
published prices.

Zero-budget MM orders are also disabled in the retained-cash oracle. The
variational identity assumes positive budget; otherwise those orders create
free, degenerate atoms even though the landed budget allows no expenditure.

## Complete development result

The following is the final complete matrix on one development machine. Runtime
statistics condition on successful, verifier-valid rows; the success column
keeps failures visible.

| Metric | LP-SLP | RC-FW | Pacing bundle | Clarabel Quasi |
|---|---:|---:|---:|---:|
| Successful / declared | 111 / 111 | 110 / 111 | 111 / 111 | 109 / 111 |
| Core termination | 87 converged, 24 capped | 82 converged, 28 capped | 111 converged | 109 converged, 2 numerical failures |
| Verifier-invalid returned candidates | 0 | 0 | 0 | 0 |
| Median wall time | 14.92 ms | 31.14 ms | 23.94 ms | 28.90 ms |
| P95 wall time | 268.71 ms | 350.28 ms | 173.23 ms | 141.78 ms |
| P99 wall time | 411.91 ms | 366.08 ms | 283.98 ms | 282.06 ms |
| Maximum wall time | 445.39 ms | 451.13 ms | 298.16 ms | 290.95 ms |
| Median observed-best retained gap | 0.1129% | 0.0000% | 0.0000% | 0.0000% |
| P95 reported continuous gap | — | $27.9763 | $0.7294 | $0.3118 |
| P95 integer landing loss | — | $4.8512 | $9.4158 | — |
| P95 minting-duality residual | $28.0197 | $0.000000035 | $0.000000689 | $0.000001447 |

The bundle retained a median of two active atoms, P95 of eight, and maximum of
13. Its restricted master was cheap on most books (47 median pairwise steps)
but not uniformly so: P95 was 6,480.5 and the maximum was 32,003. That tail is the
clearest implementation target if the method advances.

All three final failures matter. RC-FW on 16-MM seed 18403 at budget 0.25x
produced a capped core candidate, but integer landing did not reach a hard-
budget fixed point in eight steps. Clarabel returned `InsufficientProgress` on
neutral seed 16000 at budget 4x and numerical-range seed 16201 at budget 10x.
They are counted as failures and are not replaced by another solver.

The minting-duality diagnostic also distinguishes a verifier-valid allocation
from a price-supported equilibrium allocation. LP-SLP's `$28.02` P95 residual
shows why its fast median and 111/111 availability do not make it a suitable
retained-cash reference. The other three methods were below `$0.0000015` at
P95.

## Where the candidate wins and where it does not

At fixed two-MMs the bundle was faster than RC-FW at every tested order scale.
The four-way result is more informative than that pairwise comparison:

| Orders | LP-SLP ms / gap | RC-FW ms / gap | Bundle ms / gap | Clarabel ms / gap |
|---:|---:|---:|---:|---:|
| 80 | 1.95 / 21.0049% | 12.92 / 18.1420% | 3.28 / 0.3190% | 2.99 / 0.0000% |
| 400 | 8.96 / 0.2354% | 14.36 / 0.0001% | 5.60 / 0.0282% | 6.31 / 0.0000% |
| 2,000 | 25.95 / 0.3472% | 34.04 / 0.0005% | 28.03 / 0.0010% | 30.37 / 0.0000% |
| 10,000 | 276.19 / 0.3414% | 350.41 / 0.0046% | 281.22 / 0.0012% | 275.51 / 0.0000% |

The 80-order result is deliberately not softened: RC-FW's cores hit the
100-update cap, while the limited LP-SLP baseline and RC-FW landed 21.00% and
18.14% behind the observed best. The bundle reduced that to 0.319%, supporting
the intended adversarial hypothesis that retaining and correcting LP atoms
handles shared-capital face changes better than normal LP-SLP or vanilla
Frank--Wolfe. Corrected Clarabel solved the same objective exactly at displayed
precision, so this is evidence for the theoretical objective, not uniquely for
the bundle implementation.

As pacing dimension grows, the bundle advantage over RC-FW narrows. Bundle
median latency rose from 23.99 ms with one MM to 173.23 ms with 16 MMs. At 16
MMs RC-FW's conditional median was 145.74 ms but only three of four rows
succeeded; the bundle succeeded on all four with a 0.0172% mean observed-best
gap. Clarabel stayed near 32.84 ms and had a 0.0450% gap. This is evidence for
a dimension-aware policy and for replacing the bundle's pairwise master, not a
claim that the bundle dominates universally.

On the two-sided flash budget sweep RC-FW was best at 0.1x: its 0.0294% mean
observed-best retained gap beat bundle 0.1810%, LP-SLP 0.2790%, and Clarabel
0.3458%. Clarabel was best at 0.25x and 0.5x (0.0061% and 0.0007%); the bundle
recorded 0.0102% and 0.0084%, while RC-FW recorded 0.0110% and 0.0062%.
All methods matched at slack budgets. The five seeds are far too few—and too
development-exposed—for inferential claims.

## Rejected and diagnostic variants

The search history is part of the evidence:

- adding a tiny welfare tie-break to the supporting objective caused widespread
  price/limit violations and negative per-fill welfare; rejected;
- utility bands preserved the intended supporting face and improved observed
  objective values, but their omitted row duals produced a concrete
  price-above-limit conformance failure; rejected;
- supporting-objective landing without utility bands gave 111/111 bundle
  availability but lost 0.1810% at the tightest flash point because a
  degenerate supporting face selected another primal optimum; retained as the
  safe implementation and reported as a limitation;
- treating the returned LP primal as the oracle upper bound produced apparent
  certificate contradictions; corrected with the dual Lagrangian bound;
- evaluating mutated fills with the continuous cost and blindly trimming
  budgets produced deceptively good landed objectives; corrected and disclosed.

## Practical next steps

1. Keep production on RC-FW while shadow-running the bundle and recording the
   complete work/landing metrics added here.
2. Replace the pairwise restricted-master loop with a small dedicated convex
   master solve or more aggressive atom dropping only if it reduces the
   8--16-MM tail without obscuring the certificate.
3. Investigate a direct price-selection landing LP on the optimal dual face.
   The remaining bundle failure appears in price/integer recovery, not the
   continuous pacing solve; choosing a budget-feasible supporting price is more
   principled than widening primal bands indefinitely.
4. Freeze a new protocol and implementation before running untouched seeds.
   Preserve the same four solvers, orthogonal scaling axes, and every failure
   denominator; do not tune Clarabel or any other solver after seeing that set.
5. Add anonymized production replay only when a stable, consented dataset
   exists. Synthetic adversarial books should remain a separate structural
   layer rather than being relabeled realistic.

## Reproduction

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-pacing-development.json \
  --source-revision <revision> \
  --output-dir /tmp/pacing-development --overwrite

python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/pacing-development
```

The analyzer validates all 444 records, preserves failures, emits JSON/CSV and
Markdown summaries, and renders deterministic figures for retained objective,
certificates, termination, capital use, and both scaling axes.
