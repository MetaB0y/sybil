---
tags: [solver, benchmark, retained-cash, operations]
status: dated-reference
last_verified: 2026-07-13
---

# Retained-cash solver evaluation: 2026-07-13 v2

## Decision

Use `RetainedCashSolver` as the production default for batches containing
shared-capital market makers. Keep `LpSolver` as the low-latency operational
baseline, Clarabel QuasiFisher as an independent but failure-prone reference,
and SCIP only as a small-instance exact reference for the different original
hard-budget objective.

This supersedes the v1 recommendation to retain LP-SLP as the default. V1
evaluated the historical solver implementations and exposed that neither the
old IterLP fixed point nor the old forced-step EG path had the convergence
semantics their names suggested. V2 evaluates the replacement: generalized
Frank--Wolfe on the paper's exact affine-to-log retained-cash objective, with a
HiGHS matching LP as its linear oracle and an explicit continuous
suboptimality certificate.

The decision is not based on a perfect benchmark result. RC-FW returned a
verifier-valid candidate in all 135/135 declared cases, but only 110 met the
configured certificate tolerance within 100 updates. The remaining 25 are
reported as iteration limits, not convergence. Overall median wall time was
37.5 ms; the 95th percentile was 2.13 s and the maximum was 2.92 s. Those tails
need operational monitoring and probably an adaptive iteration/time policy.

## What changed

### The solver

For MM `k`, let `U_k(q)` be its value after converting every ask at limit `L`
to a complementary-outcome buy at value `1-L`. With budget `B_k`, production
now maximizes

```text
retail(q) - minting_cost(q) + sum_k psi_Bk(U_k(q)) - sell_corrections(q)

psi_B(U) = U                         when U <= B
         = B * (1 + ln(U / B))       when U > B.
```

At an iterate `q`, each MM has the single pacing multiplier
`alpha_k = min(1, B_k / U_k(q))`. The HiGHS oracle maximizes the matching LP
with every quote of MM `k` shaded by `alpha_k`. Exact one-dimensional concave
line search produces a monotone generalized Frank--Wolfe update. The reported
FW gap is a valid upper bound on continuous retained-cash objective
suboptimality; it is not an iterate-difference heuristic.

The integer landing step caps every order by the continuous allocation, then
uses a welfare LP inside those caps to obtain verifier-supported uniform
prices. It cannot invent fill quantity and is part of the declared algorithm,
not a hidden cross-solver fallback. Any final rounding overflow is trimmed at
the verifier boundary. `IterLpSolver` and `EgSolver` are explicit compatibility
aliases to this implementation and disclose the actual algorithm in
diagnostics.

### The negative-welfare defect

The original negative all-time welfare display came from treating signed
complete-set burns as a positive cost in an analytics path. That arithmetic
was corrected before this change. V2 additionally enforces the invariant at
the aggregate boundary: new recorded platform welfare cannot decrease the
total, and a restored legacy negative platform total or hourly bucket is
clamped to zero. Thus the deployed `-$244` snapshot will become `$0` on the
first restart running this revision; valid future blocks then accumulate from
there. This is an analytics migration, not a rewrite of consensus state.

### Units and independent references

The scenario generator previously interpreted configured whole-share sizes as
raw protocol quantity units. It now applies `SHARE_SCALE`, and tests enforce
whole-share quantities. Conic variables are normalized to shares and dollars,
with MM asks using the same complementary-buy reduction. SCIP prices and money
are likewise normalized before the MIQCQP solve, then landed inside the exact
integer price intervals allowed by filled orders and market-group sums.

Clarabel failures remain failures. SCIP timeout incumbents remain timeouts.
There is no cross-solver fallback in the experiment runner.

## Frozen protocol and integrity

The implementation and protocol were frozen before the held-out seeds were
run:

- source revision: `0f0824ac892d1b9268fa45fded2004f7f9777ff7`;
- protocol: `solver-evaluation-v2-retained-cash`;
- protocol BLAKE3:
  `1f85a07b0588618911577dadb4044182d651b3870dd834182f59a3c0f7276e2c`;
- 545/545 declared rows across 135 scenario groups;
- zero missing, duplicate, unexpected, or cross-solver fingerprint-mismatched
  rows;
- analysis SHA-256:
  `ab19d98c885e96323fd593fbff5c24b63f3c349ad44e79ae1ae0ce3299f71b9b`.

Development smoke seeds are below 30000. The held-out evaluation seeds are
50000 and above and were not used while repairing the algorithms. Solver order
rotates inside each problem group after a declared warm-up. Every failure,
iteration cap, verifier failure, and numerical status remains in its declared
denominator.

The run took 50 seconds on an AMD Ryzen 7 5800X, 32 GB RAM, Linux 6.6.144, and
Rust 1.97.0. These timings are single-machine measurements, not service-level
guarantees.

The later result/report revision adds the immutable artifacts, interpretation,
and one semantics-preserving Rust 1.97 lint correction in the generator
(`rank % 2 == 0` to `rank.is_multiple_of(2)`). It does not rerun, remove, or
rewrite any observation; the frozen source revision above remains the exact
code that produced `results.jsonl`.

## Overall results

| Solver | Valid / declared | Termination | Median time | Median observed-best retained gap | P95 certificate gap |
|---|---:|---|---:|---:|---:|
| RC-FW | 135/135 | 110 converged, 25 capped | 37.5 ms | 0.0000% | 0.0374% |
| LP-SLP | 125/125 | 106 converged, 19 capped | 4.0 ms | 0.0080% | not available |
| Clarabel QuasiFisher | 114/135 | 114 converged, 21 numerical failures | 5.9 ms | 0.000017% | 0.0000089% |
| Budget-blind LP | 52/125 | 73 verifier-invalid | 2.3 ms on valid rows | 0.0000% | not available |
| SCIP hard-budget reference | 15/15 | 15 proven optima | 43.9 ms | 0.8078% | different objective |
| Clarabel no-cash Fisher | 10/10 | 10 converged | 1.7 ms | suite-specific | backend gap only |

“Observed-best retained gap” compares the landed retained-cash objective with
the best verifier-valid landed allocation returned by any declared solver on
the same problem. It is useful for cross-checking but is not an optimality
proof. RC-FW's own generalized Frank--Wolfe gap is the certificate for its
continuous iterate. Integer landing loss had median `$0`, 95th percentile
`$0.00033`, and maximum `$0.4893` across RC-FW rows.

All 135 RC-FW allocations and all 125 LP-SLP allocations respected the
independent verifier, including shared MM capital. Budget-blind LP exceeded a
budget in 73/125 rows and reached 7.448 times budget in the worst row. This is
why ordinary welfare LP cannot be the production algorithm when flash
liquidity shares capital across markets.

## Adversarial flash-liquidity evidence

The flash generator creates deterministic two-sided quote ladders. Half the
crosses consume capital through MM bids; half use MM asks, exercising the
sell-to-complementary-buy reduction. One MM budget spans every quoted market.
Only the budget multiplier changes within each paired seed.

| Budget / unconstrained MM limit value | RC-FW mean retained gap | LP-SLP mean retained gap | Budget-blind LP valid | RC-FW valid | Quasi valid |
|---:|---:|---:|---:|---:|---:|
| 0.10x | 0.0091% | 0.1284% | 0/10 | 10/10 | 9/10 |
| 0.25x | 0.0000% | 0.2783% | 0/10 | 10/10 | 7/10 |
| 0.50x | 0.0003% | 1.9862% | 0/10 | 10/10 | 10/10 |
| 1.00x | 0.0000% | 0.0000% | 10/10 | 10/10 | 7/10 |
| 2.00x | 0.0000% | 0.0000% | 10/10 | 10/10 | 10/10 |
| 10.00x | 0.0000% | 0.0000% | 10/10 | 10/10 | 8/10 |

At 0.5x, LP-SLP is not merely a little less accurate: its mean retained-cash
objective gap is 1.9862%, with paired bootstrap interval `[1.9164%, 2.0464%]`,
versus RC-FW's 0.0003% `[0.0001%, 0.0004%]`. At 0.25x the corresponding values
are 0.2783% and zero at displayed precision. These are deliberately
theory-aligned adversarial cases: an ordinary LP sees individually profitable
orders but cannot represent one endogenous price-dependent budget spanning the
whole ladder. They demonstrate the advantage without modifying instances
after observing outcomes.

When budget is slack, all valid methods recover the LP allocation/objective up
to landing noise. The 10x Clarabel failures are a useful negative result:
the mathematical problem becomes easier, but cone scaling can still fail.

## Scaling and convergence tails

| Flash scale | RC-FW valid / median time / retained gap | LP-SLP valid / median time / retained gap | Quasi valid / median successful time |
|---|---|---|---|
| 48 orders | 6/6 / 27.7 ms / 0.0001% | 6/6 / 1.7 ms / 0.3701% | 6/6 / 1.5 ms |
| 400 orders | 6/6 / 48.3 ms / 0.0000% | 6/6 / 4.6 ms / 0.3611% | 4/6 / 5.7 ms |
| 2,000 orders | 4/4 / 224.9 ms / 0.0000% | 4/4 / 38.5 ms / 0.2907% | 2/4 / 34.2 ms |

RC-FW's 25 caps are concentrated in five tight neutral books, five of six
concentrated books, four flash-sweep points, one small scaling book, all five
tight numerical-range books, and all five tight no-cash-ablation books. Its
overall certificate gap has median zero, 95th percentile `$9.25` or 0.0374% of
objective, and worst value `$430.70` or 0.1159% on a deliberately
heavy-tailed numerical-range instance. The worst capped landed allocation was
0.0818% behind the observed-best retained objective.

This supports production use only with the distinction visible: the algorithm
has asymptotic convergence and an instance-specific gap, while the configured
100-update product policy can stop early. A latency-aware controller should
budget by elapsed time and certificate, not relabel capped iterates as
converged.

## Paper-bound and exact-reference checks

The runner evaluates the paper's first instance-specific welfare bound using
the unconstrained LP allocation. For RC-FW, the observed hard-budget welfare
shortfall divided by this bound had median 0.321 and maximum 0.402 among rows
where the ratio is defined. No observed row violated the bound. This is an
empirical consistency check, not a proof and not an estimate on real order
flow.

SCIP proved all 15 tiny hard-budget MIQCQPs optimal. Its median linear-welfare
improvement over LP-SLP was 0.2604%, showing that SLP is not exact even on tiny
books. SCIP's median retained-objective gap was 0.8078%, while RC-FW's was zero
at displayed precision. This is expected rather than contradictory: SCIP is
optimizing the original linear-welfare-plus-hard-budget objective, not the
retained-cash objective. The two columns must not be mixed into one “optimal”
ranking.

## Clarabel and library choice

The weakness was not that HiGHS is the wrong library. HiGHS is fast and stable
as the linear oracle, and generalized Frank--Wolfe exploits that strength. The
old weakness was the algorithm wrapped around it: fixed-point stability is not
an objective certificate.

Clarabel is valuable as an independent formulation and is roughly 6.4 times
faster than RC-FW at the overall median among successful rows. However, 21/135
QuasiFisher declarations ended in `InsufficientProgress`: 1 neutral, 2
concentrated, 9 budget-sweep, 2 medium-scaling, 2 large-scaling, and 5 numerical
stress rows. Conditional objective statistics cannot erase that 84.4% success
rate. Improving cone equilibration or comparing another exponential-cone
backend is worthwhile research, but Clarabel is not the production default
from this evidence.

## Operational follow-ups

1. Export RC-FW termination, oracle calls, absolute/relative gap, landing loss,
   and wall time to production telemetry. Alert separately on iteration cap,
   numerical failure, verifier failure, and budget trimming.
2. Replace the fixed update cap with a configurable time-plus-gap policy. Do
   not create a silent LP fallback; a liveness fallback, if ever desired, must
   be an explicit sequencer policy with a distinct metric and block annotation.
3. Add calibrated anonymized production replays once enough order flow exists.
   Keep this synthetic suite as the structural/adversarial layer rather than
   tuning it to resemble one deployment snapshot.
4. Extend scaling beyond 2,000 flash orders and measure oracle-call cost,
   memory, landing loss, price movement, and per-iteration progress curves.
5. Test alternative conic backends/scalings on the same frozen protocol and
   disclose every failure. Do not replace the evaluation seeds or report only
   successful books.

## Artifacts

Start with the generated
[`summary.md`](../benchmarks/solver/results/2026-07-13-v2/summary.md). The result
directory contains the frozen protocol, raw JSONL, machine metadata, JSON/CSV
summaries, and six deterministic SVGs for quality, budget response, scaling,
termination, capital utilization, and certificate gaps. Raw rows are evidence
and must not be edited; all derived files can be regenerated with
`scripts/benchmarks/analyze_solver_experiments.py`.
