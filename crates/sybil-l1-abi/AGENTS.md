# `sybil-l1-abi`

Host-only Alloy bindings for the Solidity contracts.

## Boundaries

- This crate may depend on Alloy and other host Ethereum tooling.
- Do not add it to OpenVM guest dependency closures; guest-safe hashes and
  protocol types belong in `sybil-l1-protocol`.
- Contract calls, events, and structs stay aligned with `contracts/src/`.
  Binding changes require affected indexer, prover, and custody golden tests.
