---
tags: [solver, crate, fisher-market, market-maker]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-17
---

# Retained cash solver

> [!summary] In one paragraph
> `RetainedCashSolver` is the independent generalized Frank--Wolfe implementation of the paper's exact affine-to-log retained-cash objective. It uses the [[The LP Core|matching LP]] as its linear oracle, reports a continuous objective and certified upper gap, then crosses the same pacing-supported integer landing boundary as production. The production [[Solver Landscape|`ProductionSolver`]] uses the monolithic fully corrective [[Pacing Bundle Solver|pacing bundle]]; RC-FW remains a reference and injectable operational alternative.

For MM `k`, let `U_k` be its non-negative weighted fill after the
buy/sell reduction and `B_k` its budget. Ignoring an allocation-independent
constant, the objective uses

```text
psi_B(U) = U                         when U <= B
         = B * (1 + ln(U / B))      when U > B.
```

Its derivative is the pacing factor `alpha_k = min(1, B_k / U_k)`. At each
iteration the solver shades every order of one MM by this common factor and
asks HiGHS for the best feasible direction. Retail welfare and signed
complete-set mint/burn cost stay inside the oracle. Exact piecewise-concave
line search updates the allocation, including changes in the active
zero-temperature minting outcome, and the generalized Frank--Wolfe gap
supplies the stopping certificate.

The feasible LP matrix is built once per batch. Objective costs change as the
pacing factors move, while HiGHS re-optimizes from the previous basis. This is
an implementation optimization only: it leaves the oracle problem and
Frank--Wolfe certificate unchanged, while avoiding repeated sparse-model setup
and cold simplex starts in the latency tail.
Each returned allocation is summarized once into MM utilities, non-mint
welfare, and per-outcome demands; line search then works in that compact space
instead of rescanning every order at each derivative evaluation.

`LinearOracleBackend::StructuralPriceSweep` is an experimental alternative for
those direction calls. For supported one-hot single-market orders it solves
the fixed-pacing price dual by sorting hinge breakpoints and recovers a primal
optimum from the price subgradient. It checks primal/dual agreement on every
call. The backend deliberately cannot handle price-linearized budget rows,
arbitrary payoff vectors, the final supporting face, or integer landing; those
paths still use HiGHS. Development results and rejected face selectors live in
`design/solver-experiments/structural-price-sweep-oracle.md`.

## MM buys and sells

The paper reduces an MM sell of YES at `L` to a buy of NO at `1-L`. The solver
implements that reduction without rewriting the protocol order:

- the positive MM value entering `U_k` is `1-L`;
- the original-coordinate objective receives the exact `-$1` complete-set
  correction per sold share;
- the paced oracle coefficient is `alpha_k * (1-L) - 1`;
- capital is checked as `(1-p_yes) * q`, matching a complementary NO buy.

This applies to short-side liquidity. Inventory liquidations that are known not
to consume shared capital should not be enrolled in an MM constraint.

## Landing and trust boundary

The continuous iterate is not protocol state. Landing solves the final
pacing-weighted objective for prices, then selects the point on that supporting
optimal face nearest to the certified iterate in L1 distance. Keeping these as
two lexicographic solves prevents a degenerate LP basis from replacing the
certified allocation, while ensuring market prices still come only from the
original matching rows. Normally scaled books use an explicitly checked exact
face; deliberately wide billion-unit books use a `1e-8` relative near-face band
directly because HiGHS can otherwise report a materially infeasible exact face
row. Landing then compares the nearest-face, primary-basis, and certified-target
integer candidates under the primary prices. Candidates more than one
microdollar above the best minting-duality residual are excluded; among the
support-equivalent remainder, landing maximizes the actual retained-cash
objective and uses residual plus stable candidate order only as tie-breakers.
It fails explicitly when even the best residual exceeds $0.05. This is an
economic support gate within the same solver and price system, not a
cross-solver fallback.

The ordinary landing caps each order at the ceiling of its continuous fill.
That localizes integer recovery, but it can exclude a much better integer point
on a large degenerate tangent face. If the localized result fails or loses more
than one basis point of continuous retained objective, the solver conditionally
repeats the same supporting-face landing with the original order bounds
(keeping zero-budget MM orders closed). It accepts the expanded-face result
only when its verifier-ready retained objective is strictly better. The retry
does not change the certified tangent, prices, support gates, or hard-budget
checks; it selects a more integer-friendly representative of that face.

If rounded quantities exceed an MM budget at the resulting prices, the
projection adds price-linearized budget rows and resolves. It finalizes only
after the prices and quantities form a budget-consistent fixed point; exhaustion
is an explicit post-processing failure, not silent trimming or a cross-solver
fallback. Welfare is recomputed with signed mint/burn cost, and
[[Four-Layer Verification|`sybil-verifier`]] remains authoritative. Landing
loss, allocation movement, budget trimming, and minting duality are separate
diagnostics because continuous convergence alone does not certify integer
recovery.

`Converged` means the configured certified-gap tolerance was met, up to a
scale-aware few-ULP subtraction floor when independently accumulated upper and
current scores coincide mathematically. The unsnapped reported gap remains
visible. An `IterationLimit` result may still be integer-valid, but the
reported gap—not iterate stability—states how far the continuous objective
could remain from optimal. Backend and landing failures are surfaced directly.

## Where this lives

> `crates/matching-solver/src/retained_cash_solver.rs` — objective, oracle loop, exact line search, and certificate  
> `crates/matching-solver/src/lp_solver.rs` — LP oracle and integer/pricing epilogue  
> `benchmarks/solver/protocol-v2.json` — preregistered shared-capital evaluation

The Cargo feature `retained-cash` exposes this production solver and keeps its
HiGHS oracle private. The broader `lp` feature adds the public LP baseline and
other research solvers; `matching-sequencer` intentionally enables only
`retained-cash`.

## See also

- [[Solver Landscape]]
- [[Conic Solver]]
- [[Pacing Bundle Solver]]
- [[MM Budget Constraint]]
- [[Welfare Maximization]]
