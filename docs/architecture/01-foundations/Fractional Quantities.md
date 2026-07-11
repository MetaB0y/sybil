---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-07-11
---

# Fractional Quantities

Sybil supports fractional shares without introducing floating-point arithmetic. The canonical representation is fixed-point share units:

- `SHARE_SCALE = 1000`
- `Qty = u64` counts share units, not whole shares
- `1000` units = 1 full YES or NO share
- `1` unit = 1/1000 share, the minimum tradable increment

This keeps matching, settlement, verification, and API payloads deterministic. The user-facing phrase "0.001 shares" is display formatting only; the protocol value is the integer `1`.

## Money Arithmetic

Prices remain `Nanos` per full share. Any time the engine converts `price * quantity` into money, it must divide by `SHARE_SCALE`:

```text
cash_nanos = price_nanos * qty_units / SHARE_SCALE
```

The multiplication must use `i128` or `u128` intermediates. Code must not silently keep the old whole-share formula, because that would overcharge fractional orders by 1000x.

Rounding is a protocol decision, not a UI decision. The default rule should be conservative:

- Buyers reserve `ceil(price * qty / SHARE_SCALE)` nanos.
- Sellers reserve `ceil(max_payoff * qty / SHARE_SCALE)` nanos where sell-side collateral is needed.
- Settlement credits and debits use deterministic floor/truncation via the shared notional helpers.
- Reservation and MM-budget checks use the conservative ceil helper so tiny fractional orders cannot overspend through rounding.

## Impact Surface

`Qty` semantics are intentionally cross-cutting:

- Order validation and reservation math
- Solver inputs and outputs
- Settlement and minting
- Resolution payouts
- Market-maker budget checks
- Verifier arithmetic
- API request/response schemas and generated clients
- Python SDK convenience helpers and frontend display formatting

The important invariant is that every protocol boundary agrees on the unit. Rust API DTO fields such as `quantity`, `max_fill`, `fill_qty`, and `remaining_quantity` contain share-units. User-facing clients convert between display shares and protocol units at their boundary.

## API Shape

The cleaner long-term API would be explicit:

- Request field: `quantity_units`
- Response field: `fill_qty_units`
- Optional convenience display fields may expose decimal shares, but they are non-authoritative.

If compatibility with external clients becomes important, add explicit `*_units` aliases or versioned DTOs rather than silently treating `quantity` as whole shares.

## Test Requirements

Before enabling fractional orders in production:

- Unit tests for `price_qty_to_nanos_floor` and `price_qty_to_nanos_ceil`.
- Settlement tests with 1, 999, 1000, and 1001 units at prices that do not divide evenly by 1000.
- Reservation tests proving buyers and sellers cannot overspend through rounding.
- Verifier tests using the same helper functions as settlement or a bit-exact duplicate.
- API round-trip tests proving integer units survive JSON serialization without float conversion.

## Related Notes

- [[Nanos and Integer Arithmetic]] - numeric foundation
- [[Settlement]] - balance and position mutations
- [[Four-Layer Verification]] - independent reproduction of arithmetic
