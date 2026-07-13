---
tags: [solver, benchmark, clarabel, latency, retained-cash]
status: dated-reference
last_verified: 2026-07-13
---

# Solver engineering follow-up: oracle reuse and conic robustness

## Decision

Keep `RetainedCashSolver` as the production algorithm. Reuse its fixed HiGHS
LP model and previous basis across Frank--Wolfe oracle calls. This changes
neither the optimization problem nor the generalized Frank--Wolfe certificate,
but materially reduces the latency tail.

Keep Clarabel QuasiFisher as an independent reference, not as a production
fallback. Use the canonical perspective exponential cone, eliminate redundant
mint variables, take more conservative interior-point steps, and retain full
failure residuals. These changes improve availability substantially without
pretending that all numerical failures have disappeared.

Remove the `EgSolver` and `IterLpSolver` compatibility names. They referred to
the same retained-cash implementation and made the solver surface look broader
than it was. Historical protocol v1 remains tied to its frozen source revision.

## Integrity boundary

This is development evidence, not a new paper result. No seed at or above
50000 from the frozen v2 evaluation was used. The robustness sweep derived a
temporary protocol from v2 by subtracting 40000 from every experiment's seed
start, setting five seeds per experiment, and retaining all declared budget
points. It ran every LP, RC-FW, QuasiFisher, and declared Fisher-ablation row;
no failed book was removed.

The final sweep contained 300 rows, including 100 QuasiFisher declarations and
100 RC-FW declarations. Temporary artifacts lived outside the repository and
must not be cited as preregistered or held-out evidence. The frozen v2 artifacts
remain the only publishable run until a future protocol and implementation are
frozen before evaluating untouched seeds.

## HiGHS oracle reuse

RC-FW changes only fill-variable objective costs between oracle calls. The
position-balance matrix, variable bounds, mint structure, and feasible region
are fixed. The old implementation rebuilt that sparse model and cold-started
HiGHS for every call. The new `ReusableLpOracle` builds it once, updates column
costs, converts each solved model back to a live model, and lets HiGHS
re-optimize from its existing basis.

A paired nine-book v2 smoke run on development seeds measured:

| Metric | Cold oracle | Reused oracle |
|---|---:|---:|
| Median wall time, all 9 RC-FW rows | 141.9 ms | 31.9 ms |
| Median wall time, 6 capped rows | 951.5 ms | 153.1 ms |
| Median speedup, all rows | 5.18x | — |
| Median speedup, capped rows | 6.50x | — |
| Capped-row speedup range | 4.44x–8.53x | — |

The one tiny 16-order row was slightly slower (3.59 ms to 3.88 ms), which is
expected when fixed setup dominates. Oracle counts and termination classes did
not change. Warm simplex can select a different point on a degenerate optimal
LP face, so some capped Frank--Wolfe trajectories and landed allocations are
not byte-identical. A regression test therefore compares warm and cold oracle
objective optima after repeated cost changes instead of demanding identical
primal vectors.

This optimization reduces the cost per iteration; it does not cure the 100-step
convergence tail. A capped row remains capped, with its certified gap visible.

## Clarabel formulation and tuning audit

The old cone wrote `exp(t/B) <= U+s`, placing `1/B` on the log-variable row.
The canonical perspective form is

```text
(t_k, B_k, U_k + s_k) in K_exp

B_k exp(t_k / B_k) <= U_k + s_k,
t_k <= B_k ln((U_k + s_k) / B_k).
```

It is the same retained-cash allocation problem up to an
allocation-independent constant and keeps the structural coefficient at one.
The conic model also substitutes
`mint_m = sum_i payoff_no_mi q_i`, reducing two balance equations and one free
mint variable per market to one YES-minus-NO equation. The final projection LP
still supplies the two clearing-price duals.

Every tested numerical variant used the same 100 development QuasiFisher rows:

| Variant | Solved | Median time | P95 time | Decision |
|---|---:|---:|---:|---|
| Perspective cone, default step | 92/100 | 6.95 ms | 105.7 ms | Baseline after algebraic scaling |
| Wider equilibration range | 92/100 | 6.97 ms | 100.9 ms | Reject: no availability gain |
| Reduced mint equations | 92/100 | 6.88 ms | 97.6 ms | Keep: exact smaller model |
| Presolve disabled | 92/100 | 7.04 ms | 99.0 ms | Reject |
| Static regularization `1e-6` | 86/100 | 7.93 ms | 101.3 ms | Reject |
| Static regularization `1e-10` | 91/100 | 7.46 ms | 83.5 ms | Reject |
| Maximum step 0.95 | 96/100 | 6.09 ms | 92.1 ms | Improvement |
| Maximum step 0.90 | 96/100 | 6.80 ms | 90.6 ms | No availability gain over 0.95 |
| Maximum step 0.80 | **97/100** | 7.16 ms | 96.2 ms | **Keep** |
| Maximum step 0.70 | 96/100 | 7.57 ms | 112.4 ms | Reject |
| Faer factorization, step 0.80 | 97/100 | 7.34 ms | 103.8 ms | Reject: 37 dependencies, no gain |

The final three failures were one slack neutral book and two slack
heavy-tailed numerical-range books, all explicit `InsufficientProgress`
statuses. Failure results now retain iterations, objective values, primal and
dual gaps, and residuals. Accepting them as “almost solved,” loosening the
verifier, or substituting an LP result would be misleading.

## Is Frank--Wolfe the final algorithm?

It is the right production algorithm from the frozen evidence: it optimizes the
paper objective, has a valid certificate, uses a mature LP backend, and returns
a verifier-valid candidate on every declared v2 row. The implementation is now
reasonably compact: one objective model, one reusable LP oracle, exact scalar
line search, and one integer/pricing epilogue.

There is nevertheless a promising lower-dimensional alternative. The retained
utility has the variational identity

```text
psi_B(U) = min_{0 < alpha <= 1} alpha U - B ln(alpha).
```

This yields a convex pacing dual over one `alpha_k` per MM, with each function
evaluation supplied by the same matching LP. Since production has few MMs, a
stabilized bundle or cutting-plane method could converge in fewer oracle calls
than primal Frank--Wolfe on adversarial books. It is not yet a drop-in
replacement: the implementation must recover a primal allocation, expose a
valid primal-dual gap, handle nonsmooth LP face changes, and beat reused RC-FW
on a preregistered development protocol. Plain BFGS or projected subgradient
would sacrifice the guarantee that motivated this work.

The next algorithmic experiment should therefore compare reused RC-FW with a
certified pacing-dual bundle method, not replace RC-FW speculatively.

## Reproduction sketch

Derive a development-only protocol without consuming evaluation seeds:

```bash
jq '.protocol_id = "solver-development-followup"
    | .experiments |= map(
        .seed_start -= 40000
        | .seed_count = 5
        | .solvers |= map(select(
            . == "lp"
            or . == "retained-cash-fw"
            or . == "conic-quasi"
            or . == "conic-fisher")))
    | .experiments |= map(select(.solvers | length > 0))' \
  benchmarks/solver/protocol-v2.json > /tmp/solver-development.json

cargo run --release -p matching-sim --bin solver-experiments \
  --features milp -- \
  --protocol /tmp/solver-development.json \
  --source-revision development-working-copy \
  --output-dir /tmp/solver-development --overwrite
```

Do not commit those rows as held-out evidence. Freeze a new protocol and source
revision before any future publishable run.
