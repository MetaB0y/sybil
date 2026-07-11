# `matching-engine`

Foundational domain and arithmetic crate. It owns orders, fills, markets,
payoff vectors, MM constraints, the solver `Problem`, and shared settlement
helpers. It contains no optimizer, persistence, transport, or actor logic.

## Read first

- [[Payoff Vectors]]
- [[Binary Markets and Market Groups]]
- [[Nanos and Integer Arithmetic]]
- [[Order Types]]
- [[Settlement]]

## Boundaries

- Protocol money uses `Nanos(u64)`; quantity uses `Qty(u64)` with
  `SHARE_SCALE = 1000` units/share.
- Money-path arithmetic uses checked integer intermediates and explicit
  floor/ceil helpers. Do not introduce floating point into settlement,
  reservation, commitments, or verification.
- A few `f64` helpers exist for display and indicative/off-validity analysis.
  Keep them visibly outside protocol truth.
- `Order` can express up to five binary markets / 32 atomic states. Current
  production admission intentionally executes a narrower single-market subset;
  expressive builders are research/test support, not a promise of live support.
- All markets are binary. Mutually exclusive multi-outcome events use
  `MarketGroup` at matching/sequencing layers.

## Main modules

| Module | Owns |
|---|---|
| `types.rs` | Units, conversion and rounding helpers, market/side enums |
| `order.rs` | `Order`, `Fill`, conditions, direction derivation |
| `order_builder.rs` | Payoff-vector construction helpers |
| `market.rs` | Binary market types |
| `state.rs` | Atomic-state indexing |
| `mm_constraint.rs` | MM capital constraints |
| `problem.rs` | Solver problem and market groups |
| `settlement.rs` | Shared fill settlement and MINT derivation |

Run `cargo test -p matching-engine`. Add boundary/rounding tests whenever money
arithmetic changes.
