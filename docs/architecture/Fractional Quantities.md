---
tags: [concept, economics]
layer: core
status: planned
last_verified: 2026-06-30
---

# Fractional Quantities

Sybil should support fractional shares without introducing floating-point arithmetic. The canonical representation is fixed-point share units:

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
- Settlement credits and debits use the same deterministic rounding helper for both parties.
- Residual rounding dust is explicitly accounted for in the block or fee/dust accumulator, never hidden in an account balance.

## Impact Surface

Changing `Qty` semantics is intentionally a cross-cutting migration:

- Order validation and reservation math
- Solver inputs and outputs
- Settlement and minting
- Resolution payouts
- Market-maker budget checks
- Verifier arithmetic
- API request/response schemas and generated clients
- Python SDK convenience helpers and frontend display formatting

The important invariant is that every protocol boundary agrees on the unit. The early-dev preference is to rename external fields or add explicit aliases if needed rather than rely on ambiguous `quantity` semantics.

## API Shape

The clean long-term API is explicit:

- Request field: `quantity_units`
- Response field: `fill_qty_units`
- Optional convenience display fields may expose decimal shares, but they are non-authoritative.

If compatibility with existing clients matters during rollout, the server can accept old `quantity` as whole shares only behind an explicit version gate. Otherwise, the migration should fail fast on ambiguous payloads.

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
