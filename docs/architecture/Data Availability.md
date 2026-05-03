---
tags: [zk, validium, data-availability]
layer: verification
status: current
last_verified: 2026-05-03
---

# Data Availability

Sybil is a validium: L1 verifies state transitions, but block data lives
off-chain. Validity and availability are therefore separate guarantees. The
state-transition proof says the new root follows from the private witness; the
DA commitment says which block payload the operator claims was made available.

The current implementation binds a concrete DA envelope into the OpenVM public
input hash. It does not yet publish to Celestia, EigenDA, Arweave, blobs, or a
committee. Provider publication is a follow-up layer over the commitment
defined here.

## Commitment

`StateTransitionPublicInputs.da_commitment` is:

```text
BLAKE3(
  "sybil/da-commitment/v1" ||
  block_height_le_u64 ||
  state_root ||
  witness_root ||
  payload_root ||
  payload_len_le_u64 ||
  provider_refs_hash
)
```

For the current witness payload:

```text
payload_root = BLAKE3(
  "sybil/da/witness-payload/v1" ||
  payload_len_le_u64 ||
  canonical_witness_bytes
)

provider_refs_hash = BLAKE3("sybil/da/provider-refs/empty/v1")
```

`canonical_witness_bytes` is the same canonical `BlockWitness` byte string
used by `witness_root`. The OpenVM guest recomputes the DA commitment from the
private witness and rejects any public input that does not match. The L1
settlement contract stores the proven value in `RootRecord.daCommitment`; it
does not attempt to judge whether the referenced data is actually available.

This makes the public root record provider-neutral. A future publisher can add
provider references without changing the state root, witness root, or payload
root semantics.

## Provider References

Provider references are intentionally outside the first circuit. A production
publisher should create a manifest that includes:

- the block height and state root
- `witness_root`, `payload_root`, and payload byte length
- a payload encoding version
- one or more provider references, such as object-store keys, DA network
  namespaces and blob IDs, or archive transaction IDs
- optional retrieval checksums and compression metadata

Before `provider_refs_hash` becomes non-empty, provider references must have a
canonical byte encoding and deterministic ordering. That lets the prover bind
the exact references while keeping L1 storage to one bytes32 field.

## Availability Model

The first useful deployment target is file-backed publication: persist the
canonical witness payload and manifest locally or in object storage, then prove
the block with the matching `da_commitment`. This gives operators and external
watchers an unambiguous audit handle without forcing an early choice of DA
network.

This is not an escape hatch by itself. If the operator disappears, users need
access to the latest enough validium state to reconstruct balances, positions,
open orders, unresolved markets, withdrawal leaves, and market metadata.
Encrypted snapshots, committee custody, MPC decryption, blobs, or a dedicated
DA layer may be needed for that future operator-replacement path. The envelope
above does not conflict with those designs because it commits to the payload
and provider-reference set without prescribing who stores or decrypts it.

## Verification Boundary

The proof verifies:

- the DA commitment matches the private witness bytes
- the witness root, state root, events root, deposit root, and block hash are
  all bound into the same public input hash
- L1 accepted the exact public input hash verified by OpenVM

The proof does not verify:

- that a DA provider retained the payload
- that a future operator can decrypt emergency snapshots
- that unresolved positions can be safely exited without normal market
  resolution

Those are availability and recovery protocol questions, tracked separately
from state-transition correctness.

## See Also

- [[Block Witness]] - private payload committed by the DA envelope.
- [[ZK Integration Path]] - OpenVM public-input binding.
- [[L1 Settlement and Vault]] - on-chain root storage.
- [[State Root Schema]] - complete validium state committed by each root.
- [[Persistence]] - storage and retention responsibilities.
