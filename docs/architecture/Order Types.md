---
tags: [concept]
layer: api
crate: sybil-api
status: current
last_verified: 2026-03-15
---

Users submit orders through the [[REST API]] using the `OrderSpec` enum — a user-friendly representation that gets converted to [[Payoff Vectors]] internally. The conversion happens at the API boundary: the user thinks in terms of "buy YES on market 3 at 55 cents for 10 shares," and the system converts this to a payoff vector over atomic states with a limit price in [[Nanos and Integer Arithmetic|nanos]].

The order types span from simple to fully custom. **BuyYes/BuyNo/SellYes/SellNo** are single-market limit orders — the bread and butter of prediction market trading. **Spread** orders simultaneously buy YES on one market and sell YES on another, expressing a relative-value view ("A is more likely than B"). **BundleYes/BundleSell** bet on conjunctions — "both A and B will happen" — across multiple markets. **Custom** is the escape hatch: the user specifies raw `market_ids`, `payoffs` arrays, and a limit price, constructing an arbitrary [[Payoff Vectors|payoff vector]] directly.

The conversion to payoff vectors is the key architectural insight. The solver never sees "BuyYes" or "Spread" — it just sees vectors and limit prices. This means adding a new order type (say, a butterfly or a conditional) only requires writing a new conversion function, not modifying the solver. The order builder in `matching-engine` provides factory functions for common patterns: `simple_yes_buy`, `spread`, `bundle_yes`, `butterfly`, `conditional_buy`. Each produces the same `Order` struct with a payoff array — the solver is agnostic to how it was created.

## Key Properties
- `OrderSpec` enum: BuyYes, BuyNo, SellYes, SellNo, Spread, BundleYes, BundleSell, Custom
- Converted to [[Payoff Vectors]] at the API boundary
- Solver is agnostic to order type — only sees payoff vectors + limit prices
- Custom orders allow arbitrary payoff vector construction
- New order types only need a conversion function, not solver changes

## Where This Lives
> `crates/sybil-api/src/types/` — `OrderSpec` request type
> `crates/sybil-api/src/convert.rs` — API-to-engine type conversion
> `crates/matching-engine/src/order_builder.rs` — factory functions for common patterns

## See Also
- [[Payoff Vectors]] — the internal representation all order types convert to
- [[REST API]] — where orders are submitted
- [[Binary Markets and Market Groups]] — market structure orders operate over
