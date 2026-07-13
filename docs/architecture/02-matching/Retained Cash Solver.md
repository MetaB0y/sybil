---
tags: [solver, crate, fisher-market, market-maker]
layer: solver
crate: matching-solver
status: current
last_verified: 2026-07-13
---

# Retained cash solver

> [!summary] In one paragraph
> `RetainedCashSolver` is the production default for shared-capital market makers. It maximizes the paper's exact affine-to-log retained-cash objective with generalized Frank--Wolfe, using the [[The LP Core|HiGHS matching LP]] as its linear oracle. It reports a continuous objective and a certified Frank--Wolfe upper gap on suboptimality, then lands the allocation into integer protocol quantities and obtains verifier-supported uniform prices with a capped welfare LP.

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
complete-set mint/burn cost stay inside the oracle. Exact concave line search
updates the allocation, and the generalized Frank--Wolfe gap supplies the
stopping certificate.

The feasible LP matrix is built once per batch. Objective costs change as the
pacing factors move, while HiGHS re-optimizes from the previous basis. This is
an implementation optimization only: it leaves the oracle problem and
Frank--Wolfe certificate unchanged, while avoiding repeated sparse-model setup
and cold simplex starts in the latency tail.

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

The continuous iterate is not protocol state. Landing caps each order at the
ceiling of its continuous fill and solves an ordinary welfare LP inside those
caps. That epilogue chooses integer-grid fills and uniform prices supported by
the original limits; it is not a fallback core solver. Any rounding-induced MM
overflow is trimmed, welfare is recomputed with signed mint/burn cost, and
[[Four-Layer Verification|`sybil-verifier`]] remains authoritative.

`Converged` means the configured certified-gap tolerance was met. An
`IterationLimit` result may still be integer-valid, but the reported gap—not
iterate stability—states how far the continuous objective could remain from
optimal. Backend and landing failures are surfaced directly.

## Where this lives

> `crates/matching-solver/src/retained_cash_solver.rs` — objective, oracle loop, exact line search, and certificate  
> `crates/matching-solver/src/lp_solver.rs` — LP oracle and integer/pricing epilogue  
> `benchmarks/solver/protocol-v2.json` — preregistered shared-capital evaluation

## See also

- [[Solver Landscape]]
- [[Conic Solver]]
- [[MM Budget Constraint]]
- [[Welfare Maximization]]
