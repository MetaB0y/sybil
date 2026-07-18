---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-18
---

The Conic Solver uses Clarabel, a Rust-native interior-point solver for conic
programs. It supports three configurable objective modes: Linear (equivalent to
a plain LP), Fisher (the forced-spend no-cash ablation), and QuasiFisher (the
paper's retained-cash formulation). QuasiFisher adds a cash variable `s_k` to
each MM's utility and is an independent reference for [[Retained Cash Solver]].

In the Fisher formulation, the objective includes `B_k * ln(U_k)` where `U_k`
is the MM's utility from fills. If an MM does not participate, `U_k` approaches
zero and the logarithm diverges. Clarabel can then report
`InsufficientProgress`. QuasiFisher replaces this with
`B_k * ln(U_k + s_k) - s_k`, where `s_k >= 0` is retained cash. The cash buffer
generally improves conditioning but does not guarantee numerical success on
every generated book. The `-s_k` term is the cash opportunity cost that yields
the affine-to-log reduced form when `s_k` is optimized out.

QuasiFisher corresponds to the theoretical cash-variable formulation and is an
independent reference for [[Retained Cash Solver]]. Quantities are normalized
to whole shares and money to dollars. Each MM uses the canonical perspective
cone `(t_k, B_k, U_k + s_k) in K_exp`, which represents
`t_k <= B_k ln((U_k+s_k)/B_k)` without a numerically harmful `1/B_k`
coefficient. The zero-temperature supply term uses the same minting epigraph as
the paper and LP oracle: independent binaries retain `M_m >= D_yes,D_no`, while
a mutually exclusive group uses
`sum_m D_no,m + max(0,max_m(D_yes,m-D_no,m))`. The earlier equality reduction
was valid only on balanced-demand faces and has been removed. MM asks use the
same sell-to-complementary-buy reduction as production.

Clarabel uses a conservative `0.8` maximum interior-point step. This improved
development-sweep availability, but it does not turn every ill-scaled generated
book into a solved instance. Non-solved statuses remain visible with iteration,
objective-gap, and residual diagnostics; they are never replaced by an LP
allocation. Solved continuous allocations pass through the shared supporting
LP and [[LP Duality and Clearing Prices|canonical integer price selection]].
Clarabel may identify the allocation face, but its numerically selected dual
point is never published. This isolates its known numerical availability
failures from protocol price determinism without hiding them.

## Key Properties
- Clarabel interior-point solver (Rust-native, conic)
- Three modes: Linear, Fisher, QuasiFisher
- QuasiFisher = `B_k * ln(U_k + s_k) - s_k` (Theorem 5)
- Cash variable `s_k` buffers the log argument away from zero when cash remains
- Perspective exponential-cone scaling and exact minting-epigraph inequalities
- Non-solved Clarabel statuses are surfaced as numerical failures; they are not
  silently replaced by LP allocations

## Where This Lives
> `crates/matching-solver/src/conic_solver.rs` — conic formulation with configurable objective modes

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[Retained Cash Solver]] — production oracle algorithm for the same objective
- [[MM Budget Constraint]] — the economic constraint being handled
