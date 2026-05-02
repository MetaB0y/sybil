---
tags: [zk]
layer: verification
status: planned
last_verified: 2026-05-02
---

Sybil is designed for a Validium architecture: off-chain data, on-chain proofs. The exchange runs off-chain for performance, but every batch's correctness is attested by an OpenVM proof posted to Ethereum L1. The [[L1 Settlement and Vault|on-chain contracts]] store accepted [[State Root and Parent Hash|state roots]], custody collateral, and process proof-backed withdrawals — they never see individual orders, fills, or account balances.

The path from current architecture to ZK proofs is deliberately incremental. The [[Four-Layer Verification|4-layer verification logic]] already exists and runs on every batch in tests. This same logic — match validity, settlement correctness, block integrity, order validation — is exactly what the ZK circuit will enforce. The [[Block Witness]] is designed as the circuit's input: a self-contained package of everything needed to verify a state transition. OpenVM is the chosen proving stack: the guest program is Rust, and the L1 contracts verify proofs through an OpenVM Solidity adapter. The current OpenVM integration is pinned to the 2.0 prerelease line, currently `v2.0.0-beta.2`.

Several architectural choices were made specifically for ZK-friendliness. [[Nanos and Integer Arithmetic|All-integer arithmetic]] maps directly to finite field operations (no floating-point emulation needed). The [[State Root Schema|state commitment]] uses a SHA-256 qMDB root so membership/exclusion checks can be wrapped in settlement and withdrawal proofs without forcing Solidity to understand qMDB directly. The [[Payoff Vectors|payoff vector]] representation keeps orders as small fixed-size arrays rather than variable-length structures, simplifying circuit layout. The verification layers are independent, allowing the circuit to be decomposed and parallelized. The OpenVM guest boundary now exists; prover orchestration, proof service operations, and DA semantics are still future work. The rollout is planned in four phases:

1. **Phase 1 (current):** 4-layer verification logic runs in Rust, exercised in tests and `matching-sim`. No ZK circuits yet.
2. **Phase 2 (started):** Compile the verification logic into an OpenVM guest program. The current guest verifies public input binding, post-state qMDB proofs, event-root recomputation, witness-root binding, and match/settlement/order logic. End-to-end proof generation is the remaining work in this phase.
3. **Phase 3:** Prover service that takes a `BlockWitness` and produces an OpenVM proof per batch. Runs alongside the sequencer.
4. **Phase 4:** [[L1 Settlement and Vault|L1 settlement and vault contracts]] on Ethereum. Store accepted state roots, verify proofs on-chain, custody deposits, and process conservative proof-backed exits; full operator disappearance recovery depends on the DA/operator replacement design.

## Current OpenVM Boundary

The first guest boundary is intentionally narrow:

- `crates/sybil-zk/` owns the public input binding shared by host tests and
  the guest. Its `guest_commitments` module contains the OpenVM-safe
  qMDB/event-root verifier subset.
- `crates/sybil-witgen/` owns host-side prover input construction. Its core
  API is a serializable `StateTransitionProofJob`: a committed
  `BlockWitness`, job identity metadata, and ordered post-state qMDB proofs.
  The default `sequencer-store` feature adds the adapter that collects this
  job from sequencer storage; the core job-to-guest conversion has no
  dependency on `matching-sequencer`.
- `crates/sybil-verifier::commitments` owns the canonical state, event, and
  witness byte schemas used by native verification, witgen, and the guest.
- `zk/openvm-guest/` is a standalone OpenVM package pinned to
  `v2.0.0-beta.2`. It is outside the root Cargo workspace so normal Rust
  checks do not require the OpenVM prerelease CLI or generated artifacts.
- The guest reads `StateTransitionGuestInput`, derives the canonical typed
  state leaves from the block witness, verifies ordered-current-qMDB
  key/value proofs for those leaves against the public `new_state_root`,
  verifies that each qMDB `next_key` pointer forms the exact sorted key ring,
  recomputes the keyless-qMDB `events_root` from canonical event leaf bytes,
  recomputes `witness_root = BLAKE3("sybil/witness" || witness_bytes)`,
  then verifies the match, settlement, and order-validation layers through
  `sybil-verifier` with qMDB block-runtime features disabled. The guest uses
  small local SHA-256/MMR verifiers for the qMDB proof/root shapes so OpenVM
  does not need to link commonware storage or its native cryptography
  dependencies.
- The guest reveals
  `keccak256(abi.encode("sybil/openvm/state-transition/v1", ...))` as the
  public value expected by `SybilSettlement`.
- Historical qMDB paths and DA binding remain follow-up work. The current
  state-root proof is a post-state exact-keyspace proof: every witness leaf
  must be in qMDB, and hidden extra leaves are rejected because they alter the
  verified `next_key` ring.
- `da_commitment` is currently a pre-DA placeholder. The only accepted value
  is zero (`PRE_DA_COMMITMENT_PLACEHOLDER`); the OpenVM guest and
  `SybilSettlement` reject any other value until SYB-76 defines provider
  semantics.

Commands:

```bash
just openvm-install
just openvm-guest-check
just openvm-guest-build
```

## Key Properties
- Validium: off-chain data, on-chain proofs
- OpenVM: Rust guest program with on-chain verification through the Solidity SDK
- State roots on Ethereum L1 — proofs attest each state transition
- Escape hatch: conservative exits plus DA-backed recovery design
- All architectural choices (integer arithmetic, typed state roots, fixed-size arrays) are ZK-motivated
- Status: planned, not yet implemented

## Where This Lives
> `crates/sybil-verifier/` — verification logic that will become the ZK circuit
> `crates/sybil-witgen/` — portable proof job type and OpenVM guest input construction
> `crates/sybil-zk/` — public input hash and guest-safe transition verifier
> `zk/openvm-guest/` — OpenVM 2.0 beta guest entrypoint
> `crates/matching-sequencer/src/qmdb_state.rs` — persisted typed-state qMDB roots and proofs used by witgen

## See Also
- [[Proof Architecture]] — authenticated data layer for arbitrary account-level proofs
- [[Four-Layer Verification]] — the checks that become the circuit
- [[Block Witness]] — the circuit's input
- [[State Root and Parent Hash]] — anchors the on-chain proof chain
- [[L1 Settlement and Vault]] — contract boundary for accepted roots and bridge custody
- [[Canonical Serialization]] — byte layout the circuit consumes
- [[Nanos and Integer Arithmetic]] — ZK-friendly arithmetic
