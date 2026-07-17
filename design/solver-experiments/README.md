# Solver experiment ledger

This directory is the durable memory for solver ideas that may never ship.
Keep failed variants as well as successful ones so future work does not repeat
an attractive dead end after its context has disappeared.

Each experiment entry records:

- a stable experiment ID and date;
- the falsifiable hypothesis and mathematical variant;
- source revision, command, workload, and seed boundary;
- complete outcome, including failures and counterexamples;
- the decision and the condition under which it should be revisited.

Development results guide engineering only. Held-out claims still require a
protocol and source revision frozen before untouched seeds are evaluated.

## Active ledgers

- [Direct price–pacing dual](price-pacing-dual.md) — issue
  [#173](https://github.com/MetaB0y/sybil/issues/173)
- [Structural price-sweep matching oracle](structural-price-sweep-oracle.md) —
  exact fixed-pacing primal/dual oracle and marginal-face recovery variants
- [Exact economic-connectivity decomposition](exact-component-decomposition.md)
  — successful balanced-component router, replay topology audit, and frozen
  adversarial one-component production audit
- [Public CLOB-depth corpus](public-clob-depth-corpus.md) — frozen external
  resting depth, observed taker-flow windows, explicit synthetic controls, and
  benchmark calibration
- [Full tangent-face integer landing](integer-face-landing-retry.md) —
  conditional integer-friendly retry on a certified retained-cash tangent
- [Direct scalar pacing dual](scalar-pacing-dual.md) — rejected certified
  one-MM bisection and safeguarded semismooth variants
