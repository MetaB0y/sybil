---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-13
---

The Conic Solver uses Clarabel, a Rust-native interior-point solver for conic programs. It supports three configurable objective modes: Linear (equivalent to a plain LP), Fisher (same as the [[EG Solver]] but via conic formulation), and QuasiFisher. The QuasiFisher mode is the interesting one — it adds a "cash variable" `s_k` to each MM's utility that fixes a numerical problem the pure Fisher formulation suffers from.

In the Fisher formulation, the objective includes `B_k * ln(U_k)` where `U_k` is the MM's utility from fills. If an MM happens to not participate in any fills (perhaps all their orders are out-of-the-money), `U_k` approaches zero and the logarithm blows up to negative infinity. This can make the exponential cone ill-conditioned and Clarabel can report `InsufficientProgress`. The QuasiFisher mode replaces this with `B_k * ln(U_k + s_k) - s_k`, where `s_k >= 0` is a cash variable representing unspent budget. This generally improves conditioning by providing a cash buffer, but it does not guarantee numerical success on every generated book. The `-s_k` penalty ensures MMs still prefer spending over hoarding cash.

QuasiFisher corresponds to the theoretical cash-variable formulation; see `design/math-papers.md`. The solver is useful as an independent convex formulation. Current performance and welfare gaps belong in reproducible benchmark output.

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
- [[EG Solver]] — pure Fisher formulation without cash variable
- [[MM Budget Constraint]] — the economic constraint being handled
