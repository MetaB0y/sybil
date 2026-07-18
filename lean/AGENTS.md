# `lean`

Formal research model for Fisher-market clearing; it is not executable protocol
truth or a solver-conformance oracle.

- Keep theorem assumptions explicit when relating results to production
  admission, integer landing, MM budgets, or verification.
- Do not mechanically change the model to mirror an implementation result.
- Preserve the pinned Lean/mathlib toolchain and run `lake build`.
