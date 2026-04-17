---
tags: [concept, infrastructure]
layer: core
crate: matching-engine
status: current
last_verified: 2026-03-15
---

Sybil uses no floating-point arithmetic anywhere in the system. All prices and quantities are represented as unsigned 64-bit integers in "nanos" — nanodollars, where 1 dollar equals 1,000,000,000 (10^9) nanos. This gives sub-cent precision (9 decimal places) while keeping all computation deterministic and ZK-friendly.

The choice is driven by two requirements. First, determinism: floating-point arithmetic is notoriously platform-dependent. The same f64 calculation can give different results on different CPUs, compilers, or optimization levels. For a system that needs verifiable computation — where an independent [[Four-Layer Verification|verifier]] must reproduce the exact same state transitions — this is unacceptable. Integer arithmetic is identical everywhere. Second, ZK-friendliness: arithmetic circuits for SNARK proofs operate over finite fields, which map naturally to integer operations but require expensive emulation for floating-point.

A u64 in nanos can represent values up to about $18.4 billion, which is ample for any realistic prediction market. For intermediate calculations during [[Settlement]] — particularly when multiplying price by quantity — the system uses i128 to avoid overflow. The signed type handles the fact that settlement involves both debits and credits. All external-facing values (API requests, block data) are in nanos, and the [[Python SDK]] provides convenience methods to convert between dollars and nanos.

## Key Properties
- `Nanos` = u64, 1 dollar = 1,000,000,000 nanos
- `Qty` = u64, share quantities
- i128 intermediates in [[Settlement]] for overflow safety
- Maximum representable value: ~$18.4B (u64::MAX / 10^9)
- Deterministic across all platforms — critical for [[Four-Layer Verification|verification]]
- Maps directly to ZK arithmetic circuits

## Where This Lives
> `crates/matching-engine/src/types.rs` — `Nanos`, `Qty`, `NANOS_PER_DOLLAR` constant

## See Also
- [[Settlement]] — where i128 intermediates prevent overflow
- [[ZK Integration Path]] — why integer arithmetic matters for proofs
- [[Four-Layer Verification]] — verification requires deterministic arithmetic
- [[Canonical Serialization]] — how these integers are turned into bytes
