# `sybil-polymarket`

Untrusted Polymarket integration for curated mirroring, reference prices,
flash liquidity, and attributable source resolution. It talks to Sybil only
through shared clients/types/signing.

## Read first

- [[REST API]], [[WebSocket Block Stream]], and [[Market Resolution]]
- [[Order Admission]] and [[MM Budget Constraint]]

## Boundaries

- Validate every Gamma/CLOB identifier, shape, nested JSON field, and lifecycle
  claim. A signed source is attributable, not objectively true.
- Mapping and display/reference metadata are off-block and never validity
  inputs.
- The atomically replaced mapping is schema-versioned and bound to one Sybil
  genesis; mismatches fail startup.
- Preserve source provenance on mirrored markets.
- MM liquidity is one-shot/IOC and uses the shared `sybil-market-maker`
  runtime; it must not become a second internal order book.
- Authenticated actions use canonical signing bytes and monotonic nonces.
- Secrets come from environment/files and never enter logs or tracked state.
- Runtime actors publish progress to the owning process's private health and
  metrics boundary. API reference-price expiry is independent of actor health.
- `curated_markets.json` owns mirrored event selection. Native catalog,
  liquidity, and resolution belong to `sybil-native`.

Use Clap `--help` for runtime configuration. Run crate tests plus clippy after
integration changes.
