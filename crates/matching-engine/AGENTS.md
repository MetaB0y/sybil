# `matching-engine`

Foundational orders, markets, payoff vectors, MM constraints, solver input, and
shared settlement arithmetic. It owns no optimizer, transport, persistence, or
actor behavior.

## Read first

- [[Payoff Vectors]], [[Binary Markets and Market Groups]], and [[Order Types]]
- [[Nanos and Integer Arithmetic]] and [[Settlement]]

## Boundaries

- Money is `Nanos(u64)`; quantity is `Qty(u64)` with `SHARE_SCALE = 1000`.
- Protocol arithmetic uses checked integer intermediates and explicit rounding.
  Existing `f64` helpers remain display or off-validity analysis only.
- `Order` can represent broader multi-market shapes than production admission
  executes. Builders and solver support are not a live-product promise.
- Markets are binary; mutually exclusive outcomes are represented by
  `MarketGroup` above the individual-market layer.
- Money-path changes require boundary and rounding tests, not only happy-path
  examples.
