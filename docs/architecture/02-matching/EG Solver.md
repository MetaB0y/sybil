---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-13
---

`EgSolver` is now an explicit compatibility alias to [[Retained Cash Solver|`RetainedCashSolver`]]. The previous implementation used a forced-step, no-cash Frank--Wolfe variant and did not provide the convergence semantics its name suggested.

The retained-cash solver implements the quasilinear Fisher interpretation used
by the paper: MM utility is affine while its budget is slack and logarithmic
after capital binds. The no-cash `B_k ln U_k` objective remains available only
as `ConicSolver`'s `Fisher` ablation, where forced spending is intentional and
visible.

Calls through the old type report `retained-cash-fw` in diagnostics and state
that the legacy name is an alias. There is no silent cross-solver fallback.

## Key Properties
- Compatibility surface only
- Actual implementation and certificate live in [[Retained Cash Solver]]
- No-cash Fisher ablation lives in [[Conic Solver]]

## Where This Lives
> `crates/matching-solver/src/eg_solver.rs` — explicit compatibility wrapper

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[MM Budget Constraint]] — the constraint absorbed by retained-cash utility
- [[Conic Solver]] — independent conic formulation and no-cash ablation
