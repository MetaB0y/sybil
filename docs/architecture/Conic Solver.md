---
tags: [solver, crate]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-03-15
---

The Conic Solver uses Clarabel, a Rust-native interior-point solver for conic programs. It supports three configurable objective modes: Linear (equivalent to a plain LP), Fisher (same as the [[EG Solver]] but via conic formulation), and QuasiFisher. The QuasiFisher mode is the interesting one — it adds a "cash variable" `s_k` to each MM's utility that fixes a numerical problem the pure Fisher formulation suffers from.

In the Fisher formulation, the objective includes `B_k * ln(U_k)` where `U_k` is the MM's utility from fills. If an MM happens to not participate in any fills (perhaps all their orders are out-of-the-money), `U_k` approaches zero and the logarithm blows up to negative infinity. This makes the exponential cone ill-conditioned and Clarabel reports `InsufficientProgress`. The QuasiFisher mode replaces this with `B_k * ln(U_k + s_k) - s_k`, where `s_k >= 0` is a cash variable representing unspent budget. This keeps the argument of the log bounded away from zero (`U_k + s_k >= s_k > 0`) and acts as a numerical buffer. The `-s_k` penalty ensures MMs still prefer spending over hoarding cash.

This is Theorem 5 from the theoretical work (see `lmsr-proof.typ`). In benchmarks, QuasiFisher achieves welfare within ~0.5% of the [[LP Solver]], nearly matching it in quality, while being theoretically cleaner about budget handling. The welfare gap stays below 0.7% across all budget scaling levels (from 0.1x to 1.5x the default budget), much tighter than the quadratic bound from Proposition 5 would predict. Known issue: at budget scales >= 2x, the cash variable grows large and Clarabel's convergence degrades.

## Key Properties
- Clarabel interior-point solver (Rust-native, conic)
- Three modes: Linear, Fisher, QuasiFisher
- QuasiFisher = `B_k * ln(U_k + s_k) - s_k` (Theorem 5)
- Cash variable `s_k` prevents log-of-zero numerical blowup
- ~0.5% welfare gap vs [[LP Solver]] (see `design/solver-benchmarks.md`)
- Welfare gap < 0.7% across all budget scales

## Where This Lives
> `crates/matching-solver/src/conic_solver.rs` — conic formulation with configurable objective modes

## See Also
- [[Solver Landscape]] — comparison with other solvers
- [[EG Solver]] — pure Fisher formulation without cash variable
- [[MM Budget Constraint]] — the economic constraint being handled
