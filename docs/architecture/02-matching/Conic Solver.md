---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-11
---

The Conic Solver uses Clarabel, a Rust-native interior-point solver for conic programs. It supports three configurable objective modes: Linear (equivalent to a plain LP), Fisher (same as the [[EG Solver]] but via conic formulation), and QuasiFisher. The QuasiFisher mode is the interesting one — it adds a "cash variable" `s_k` to each MM's utility that fixes a numerical problem the pure Fisher formulation suffers from.

In the Fisher formulation, the objective includes `B_k * ln(U_k)` where `U_k` is the MM's utility from fills. If an MM happens to not participate in any fills (perhaps all their orders are out-of-the-money), `U_k` approaches zero and the logarithm blows up to negative infinity. This makes the exponential cone ill-conditioned and Clarabel reports `InsufficientProgress`. The QuasiFisher mode replaces this with `B_k * ln(U_k + s_k) - s_k`, where `s_k >= 0` is a cash variable representing unspent budget. This keeps the argument of the log bounded away from zero (`U_k + s_k >= s_k > 0`) and acts as a numerical buffer. The `-s_k` penalty ensures MMs still prefer spending over hoarding cash.

QuasiFisher corresponds to the theoretical cash-variable formulation; see `design/math-papers.md`. The solver is useful as an independent convex formulation. Current performance and welfare gaps belong in reproducible benchmark output.

## Key Properties
- Clarabel interior-point solver (Rust-native, conic)
- Three modes: Linear, Fisher, QuasiFisher
- QuasiFisher = `B_k * ln(U_k + s_k) - s_k` (Theorem 5)
- Cash variable `s_k` prevents log-of-zero numerical blowup

## Where This Lives
> `crates/matching-solver/src/conic_solver.rs` — conic formulation with configurable objective modes

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[EG Solver]] — pure Fisher formulation without cash variable
- [[MM Budget Constraint]] — the economic constraint being handled
