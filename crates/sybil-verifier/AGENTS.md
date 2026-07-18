# `sybil-verifier`

Canonical native block-witness verifier and encoding owner. `sybil-zk` reuses
this transition logic, so changes can move witness bytes, state roots, guest
commitments, and L1 public inputs.

## Read first

- [[Four-Layer Verification]], [[Block Witness]], and [[State Root Schema]]
- [[Canonical Serialization]], [[P256 Authentication]], and
  [[ZK Integration Path]]

## Rules

- `verify_full` covers match/settlement, exact authenticated state,
  admission/rejections, system/bridge/lifecycle transitions, and key/action
  authorization. `diagnostics` changes reporting, never validity.
- Never fork canonical layouts between host and guest.
- Preserve exact-keyspace and absence proofs, not inclusion alone.
- Use shared checked integer settlement helpers; no floating point.
- Tag, version, layout, or domain changes require golden vectors, decoders,
  guest fingerprints/commitments, current docs, and an explicit
  migration/fresh-genesis decision.
- This crate is the sole trusted solver-output verifier.

Validity-surface changes require crate tests, `just golden-check`, and the
relevant `sybil-zk`/OpenVM checks.
