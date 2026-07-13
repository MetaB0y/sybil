---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-13
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
independent reference for [[Retained Cash Solver]]. Quantities and minting are
normalized to whole shares, money to dollars, and the exponential-cone log
variable is scaled by its MM budget before Clarabel sees the problem. MM asks
use the same sell-to-complementary-buy reduction as the production solver.
Non-solved statuses remain visible in reproducible benchmark output.

## Key Properties
- Clarabel interior-point solver (Rust-native, conic)
- Three modes: Linear, Fisher, QuasiFisher
- QuasiFisher = `B_k * ln(U_k + s_k) - s_k` (Theorem 5)
- Cash variable `s_k` buffers the log argument away from zero when cash remains
- Non-solved Clarabel statuses are surfaced as numerical failures; they are not
  silently replaced by LP allocations

## Where This Lives
> `crates/matching-solver/src/conic_solver.rs` — conic formulation with configurable objective modes

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[Retained Cash Solver]] — production oracle algorithm for the same objective
- [[MM Budget Constraint]] — the economic constraint being handled
