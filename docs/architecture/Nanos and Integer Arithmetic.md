---
tags: [concept, infrastructure]
layer: core
crate: matching-engine
status: current
last_verified: 2026-07-01
---

Sybil uses no floating-point arithmetic in protocol-critical computation. Prices are represented as unsigned 64-bit integers in "nanos" — nanodollars, where 1 dollar equals 1,000,000,000 (10^9) nanos. Quantities use [[Fractional Quantities|fixed-point share-units]], where `SHARE_SCALE = 1000`. This gives sub-cent price precision and 0.001-share quantity precision while keeping all computation deterministic and ZK-friendly.

The choice is driven by two requirements. First, determinism: floating-point arithmetic is notoriously platform-dependent. The same f64 calculation can give different results on different CPUs, compilers, or optimization levels. For a system that needs verifiable computation — where an independent [[Four-Layer Verification|verifier]] must reproduce the exact same state transitions — this is unacceptable. Integer arithmetic is identical everywhere. Second, ZK-friendliness: arithmetic circuits for SNARK proofs operate over finite fields, which map naturally to integer operations but require expensive emulation for floating-point.

A u64 in nanos can represent values up to about $18.4 billion, which is ample for any realistic prediction market. For intermediate calculations during [[Settlement]] — particularly when multiplying price by quantity — the system uses i128/u128 helpers to avoid overflow. Money arithmetic is `price_nanos * qty_units / SHARE_SCALE`. The signed type handles the fact that settlement involves both debits and credits. All external-facing money values are in nanos, protocol quantity fields are in share-units, and the [[Python SDK]] provides convenience methods to convert between dollars, shares, nanos, and share-units.

## Key Properties
- `Nanos` = u64, 1 dollar = 1,000,000,000 nanos
- `Qty` = u64 fixed-point share-units; `SHARE_SCALE = 1000`
- `1000` quantity units = 1 YES/NO share; `1` quantity unit = 0.001 share
- i128/u128 intermediates in [[Settlement]] and reservation math for overflow safety
- Maximum representable value: ~$18.4B (u64::MAX / 10^9)
- Deterministic across all platforms — critical for [[Four-Layer Verification|verification]]
- Maps directly to ZK arithmetic circuits

## Where This Lives
> `crates/matching-engine/src/types.rs` — `Nanos`, `Qty`, `NANOS_PER_DOLLAR`, `SHARE_SCALE`, and notional helpers

## See Also
- [[Fractional Quantities]] — quantity unit model and rounding policy
- [[Settlement]] — where i128 intermediates prevent overflow
- [[ZK Integration Path]] — why integer arithmetic matters for proofs
- [[Four-Layer Verification]] — verification requires deterministic arithmetic
- [[Canonical Serialization]] — how these integers are turned into bytes
