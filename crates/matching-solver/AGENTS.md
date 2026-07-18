# `matching-solver`

Optimization implementations for the supported welfare-clearing problem. The
production-supported core is convex/linear; optional MILP references do not
make the whole crate NP-hard.

## Read first

- [[Solver Landscape]], [[The LP Core]], and [[Welfare Maximization]]
- [[LP Duality and Clearing Prices]] and [[MM Budget Constraint]]
- The focused note for the solver being changed

## Invariants

- Solvers may search in floating point; landed fills, prices, and trusted
  welfare are integers verified by `sybil-verifier`.
- Distinguish a MILP incumbent from a proven optimum and an iteration cap from
  convergence. Only an explicitly reported valid gap is a certificate.
- Numerical failure must be explicit; every returned candidate crosses the
  same integer landing and verifier boundary as production.
- Solver expressiveness must not broaden production admission.
- Preserve uniform price, limits, groups, MM budgets, rounding, net-of-minting
  welfare, and integer landing across implementations.
