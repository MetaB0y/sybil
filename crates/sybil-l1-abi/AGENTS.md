# AGENTS.md

## Scope

`sybil-l1-abi` owns host-only Alloy bindings for the Sybil Solidity contracts.
Keep contract calls, events, and structs aligned with `contracts/src/`.

## Boundaries

- This crate may depend on Alloy and other host Ethereum tooling.
- Do not add it to OpenVM guest dependency closures; guest-safe hashes and
  protocol types belong in `sybil-l1-protocol`.
- Preserve the existing calldata and event golden tests in consumer crates when
  changing bindings.

## Checks

```bash
cargo check -p sybil-l1-abi
cargo test -p sybil-l1-indexer -p sybil-prover -p sybil-custody
```
