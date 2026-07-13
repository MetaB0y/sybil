---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: deprecated
last_verified: 2026-07-13
---

`EgSolver` has been removed. The previous implementation used a forced-step,
no-cash Frank--Wolfe variant and did not provide the convergence semantics its
name suggested; a temporary compatibility alias was also removed once current
callers migrated to [[Retained Cash Solver|`RetainedCashSolver`]].

The retained-cash solver implements the quasilinear Fisher interpretation used
by the paper: MM utility is affine while its budget is slack and logarithmic
after capital binds. The no-cash `B_k ln U_k` objective remains available only
as `ConicSolver`'s `Fisher` ablation, where forced spending is intentional and
visible.

Historical experiments that name the old type remain reproducible from their
recorded frozen source revisions. Current code has no silent alias or
cross-solver fallback.

## Key Properties
- Removed historical surface
- Actual implementation and certificate live in [[Retained Cash Solver]]
- No-cash Fisher ablation lives in [[Conic Solver]]

## Where This Lives
> `benchmarks/solver/protocol-v1.json` — frozen historical experiment contract

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[MM Budget Constraint]] — the constraint absorbed by retained-cash utility
- [[Conic Solver]] — independent conic formulation and no-cash ablation
