---
tags: [zk, validium, data-availability]
layer: verification
status: current
last_verified: 2026-05-05
---

# Data Availability

Sybil is a validium: L1 verifies state transitions, but block data lives
off-chain. Validity and availability are therefore separate guarantees. The
state-transition proof says the new root follows from the private witness; the
DA commitment says which block payload the operator claims was made available.

The current implementation binds a concrete DA envelope into the OpenVM public
input hash for proof-pipeline testing. Its file-backed witness payload is
local scaffolding, not the final privacy-preserving validium recovery design.
Production recovery DA is expected to use encrypted snapshots or encrypted
deltas with an emergency disclosure protocol; that design is tracked in
SYB-120.

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

For the current local proof-pipeline witness payload:

```text
payload_root = BLAKE3(
  "sybil/da/witness-payload/v1" ||
  payload_len_le_u64 ||
  canonical_witness_bytes
)

provider_refs_hash =
  BLAKE3("sybil/da/provider-refs/empty/v1") when no refs are present
  or
  BLAKE3("sybil/da/provider-refs/v1" || ref_count_le_u64 || len/ref...)
```

`canonical_witness_bytes` is the same canonical `BlockWitness` byte string
used by `witness_root` (`sybil-canonical-witness-v3`, wire version byte 3). It
begins with the format-version byte and includes the pre- and post-state
account sections, the pre- and post-state sidecars, and the deposit-accumulator
section (its pre-frontier, pre-count, and new-deposit stream) — i.e. the full
private witness the guest re-derives, not just the visible fields. This plaintext witness payload is acceptable for local
smoke tests and prover orchestration, but should not be treated as production
public DA for a private validium. The OpenVM guest recomputes the DA
commitment from the private witness and rejects any public input that does not
match. The L1 settlement contract stores the proven value in
`RootRecord.daCommitment`; it does not attempt to judge whether the referenced
data is actually available.

This makes the public root record provider-neutral. A future publisher can add
provider references without changing the state root, witness root, or payload
root semantics.

## Provider References

Provider references are private guest input bytes. The proof hashes them into
`provider_refs_hash`; L1 sees only the final `daCommitment`. A production
publisher should create a manifest that includes:

- the block height and state root
- `witness_root`, `payload_root`, and payload byte length
- a payload encoding version
- one or more provider references, such as object-store keys, DA network
  namespaces and blob IDs, or archive transaction IDs
- optional retrieval checksums and compression metadata

Provider references must have a canonical byte encoding and deterministic
ordering. That lets the prover bind the exact references while keeping L1
storage to one bytes32 field.

## File-Backed Scaffold

The host tooling can prepare a proof-bound file-backed DA publication directly
from a proof job:

```bash
just prover-prepare-file-da /tmp/job.msgpack /tmp/sybil-guest-input.msgpack /tmp/sybil-da /tmp/sybil-da-manifest.json /tmp/sybil-public-input-hash.hex
```

`sybil-prover prepare-file-da` validates the proof job, writes a
`StateTransitionGuestInput`, derives a deterministic payload filename from
`payload_root`, writes canonical witness bytes under `payload_dir`, and writes
a JSON manifest. The file provider ref commits to:

- `sybil/da/provider-ref/file/v1`
- a stable content-addressed `sybil-file://witness/{payload_root}.witness.bin`
  URI
- `payload_root`
- payload byte length

The manifest includes:

- block height, block hash, state root, witness root, payload root, payload
  length, provider refs hash, DA commitment, and public-input hash
- one `provider_refs` entry with `kind: "file"` and
  `encoding: "sybil-da-file-ref-v1"`
- `local_payload_path`, which is where this host wrote the file and is not
  proof-bound

`sybil-prover publish-da` still exists for empty-ref or already-prepared guest
inputs, but the smoke path uses `prepare-file-da` so OpenVM execution and app
proofs cover a non-empty provider-reference hash. This is deliberately a
scaffold: it proves the commitment/provider-ref plumbing works without
deciding that plaintext witness publication is acceptable production DA.

## Availability Model

The first useful engineering target is file-backed publication: persist the
canonical witness payload and manifest locally, then prove the block with the
matching file provider reference. This gives prover workers an unambiguous
artifact handle without forcing an early choice of DA network.

This is not an escape hatch by itself. If the operator disappears, users need
access to the latest enough validium state to reconstruct balances, positions,
open orders, unresolved markets, withdrawal leaves, and market metadata.
Encrypted snapshots, committee custody, MPC decryption, blobs, or a dedicated
DA layer may be needed for that future operator-replacement path. The envelope
above is intended not to conflict with those designs because it commits to the
payload and provider-reference set without prescribing whether the payload is
plaintext, encrypted, local, or externally posted.

## Verification Boundary

The proof verifies:

- the DA commitment matches the private witness bytes
- the provider-reference bytes hash to the `provider_refs_hash` inside that
  DA commitment
- the witness root, state root, events root, deposit root, and block hash are
  all bound into the same public input hash
- L1 accepted the exact public input hash verified by OpenVM

The proof does not verify:

- that a DA provider retained the payload
- that plaintext witness publication is the production recovery model
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
