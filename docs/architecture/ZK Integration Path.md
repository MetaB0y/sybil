---
tags: [zk]
layer: verification
status: planned
last_verified: 2026-04-26
---

Sybil is designed for a Validium architecture: off-chain data, on-chain proofs. The exchange runs off-chain for performance, but every batch's correctness is attested by an OpenVM proof posted to Ethereum L1. The [[L1 Settlement and Vault|on-chain contracts]] store accepted [[State Root and Parent Hash|state roots]], custody collateral, and process proof-backed withdrawals — they never see individual orders, fills, or account balances.

The path from current architecture to ZK proofs is deliberately incremental. The [[Four-Layer Verification|4-layer verification logic]] already exists and runs on every batch in tests. This same logic — match validity, settlement correctness, block integrity, order validation — is exactly what the ZK circuit will enforce. The [[Block Witness]] is designed as the circuit's input: a self-contained package of everything needed to verify a state transition. OpenVM is the chosen proving stack: the guest program is Rust, and the L1 contracts verify proofs through an OpenVM Solidity adapter.

Several architectural choices were made specifically for ZK-friendliness. [[Nanos and Integer Arithmetic|All-integer arithmetic]] maps directly to finite field operations (no floating-point emulation needed). The v2 [[State Root Schema|state commitment]] uses a SHA-256 qmdb root so membership/exclusion checks can be wrapped in settlement and withdrawal proofs without forcing Solidity to understand qmdb directly. The [[Payoff Vectors|payoff vector]] representation keeps orders as small fixed-size arrays rather than variable-length structures, simplifying circuit layout. The verification layers are independent, allowing the circuit to be decomposed and parallelized. The ZK layer is currently not implemented — the architecture is ready, the circuit compilation is future work. The rollout is planned in four phases:

1. **Phase 1 (current):** 4-layer verification logic runs in Rust, exercised in tests and `matching-sim`. No ZK circuits yet.
2. **Phase 2:** Compile the verification logic into an OpenVM guest program. Prove that the same Rust code produces a valid proof.
3. **Phase 3:** Prover service that takes a `BlockWitness` and produces an OpenVM proof per batch. Runs alongside the sequencer.
4. **Phase 4:** [[L1 Settlement and Vault|L1 settlement and vault contracts]] on Ethereum. Store accepted state roots, verify proofs on-chain, custody deposits, and process conservative proof-backed exits; full operator disappearance recovery depends on the DA/operator replacement design.

## Key Properties
- Validium: off-chain data, on-chain proofs
- OpenVM: Rust guest program with on-chain verification through the Solidity SDK
- State roots on Ethereum L1 — proofs attest each state transition
- Escape hatch: conservative exits plus DA-backed recovery design
- All architectural choices (integer arithmetic, typed state roots, fixed-size arrays) are ZK-motivated
- Status: planned, not yet implemented

## Where This Lives
> `crates/sybil-verifier/` — verification logic that will become the ZK circuit
> `crates/matching-sequencer/src/block.rs` — block structure designed for ZK witness

## See Also
- [[Proof Architecture]] — authenticated data layer for arbitrary account-level proofs
- [[Four-Layer Verification]] — the checks that become the circuit
- [[Block Witness]] — the circuit's input
- [[State Root and Parent Hash]] — anchors the on-chain proof chain
- [[L1 Settlement and Vault]] — contract boundary for accepted roots and bridge custody
- [[Canonical Serialization]] — byte layout the circuit consumes
- [[Nanos and Integer Arithmetic]] — ZK-friendly arithmetic
