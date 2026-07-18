---
tags: [concept]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-18
---

Users submit orders through the [[REST API]] using the `OrderSpec` enum — a user-friendly representation that gets converted to [[Payoff Vectors]] internally. The conversion happens at the API boundary: the user thinks in terms of "buy YES on market 3 at 55 cents for 10 shares," and the system converts this to a payoff vector over atomic states with a limit price in [[Nanos and Integer Arithmetic|nanos]] and a quantity in fixed-point share-units (`10 shares = 10_000` units).

The order types span from simple to fully custom. **BuyYes/BuyNo/SellYes/SellNo** are single-market limit orders — the bread and butter of prediction market trading. **Spread** orders simultaneously buy YES on one market and sell YES on another, expressing a relative-value view ("A is more likely than B"). **BundleYes/BundleSell** bet on conjunctions — "both A and B will happen" — across multiple markets. **Custom** is the escape hatch: the user specifies raw `market_ids`, `payoffs` arrays, and a limit price, constructing an arbitrary [[Payoff Vectors|payoff vector]] directly.

The API exposes time-in-force names, but the core order model is expiry-only. `GTC` maps to no user expiry and may rest until cancelled or until the system TTL expires. `GTD` maps to a client-supplied `expires_at_block`, capped by the system TTL. `IOC` is immediate-or-cancel in FBA terms: the API maps it to the next eligible batch height, so any unfilled remainder is cancelled after that batch. Signed orders commit to the resolved `expires_at_block`; a signed IOC is therefore just a signed one-batch expiry.

The conversion to payoff vectors is the key architectural insight. The order builder in `matching-engine` provides factory functions for common patterns: `simple_yes_buy`, `spread`, `bundle_yes`, `butterfly`, and `conditional_buy`. Each produces the same `Order` struct, keeping the API and settlement model unified even where a solver supports only a subset of shapes.

> [!warning] Current clearing limit
> The API and engine can construct multi-market vectors and price conditions,
> but production clearing currently accepts only unconditional, one-market,
> binary, one-hot orders. General vectors are rejected rather than projected
> onto binary prices. Price-conditioned admission is also rejected until the
> primary allocation solver models its union of active faces. Canonical
> verification still handles an already-landed positive conditional fill by
> fixing its active branch and selecting the strict integer condition boundary.

## Key Properties
- `OrderSpec` enum: BuyYes, BuyNo, SellYes, SellNo, Spread, BundleYes, BundleSell, Custom
- Quantity fields are protocol share-units; `1000` units = 1 share
- API time-in-force: GTC, IOC, GTD; core model: optional `expires_at_block`
- Converted to [[Payoff Vectors]] at the API boundary
- The domain representation is general; the production solver/verifier subset
  is deliberately narrower
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
