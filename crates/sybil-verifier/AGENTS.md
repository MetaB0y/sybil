# `sybil-verifier`

Canonical native verifier and encoding owner for the block witness. The OpenVM
guest reuses this transition logic through `sybil-zk`; changes here can move
guest commitments, state roots, witness bytes, and L1 public inputs.

## Read first

- [[Four-Layer Verification]] and [[Block Witness]]
- [[Canonical Serialization]] and [[State Root Schema]]
- [[P256 Authentication]] and [[ZK Integration Path]]

## Verification shape

`verify_full(witness, diagnostics)` combines more than the historical four
named layers:

- match/fill, uniform-price, group, MM-budget, and welfare checks;
- integer settlement/MINT replay;
- header roots, parent/height/counts, and exact authenticated state;
- admission funding/positions/expiry/rejection validity;
- system-event, bridge/quarantine, lifecycle, and committed sidecar transition;
- active-key universe/digest transitions and RawP256/WebAuthn authorization.

There is no “lenient versus strict” verifier mode. The `diagnostics` flag
controls extra diagnostic reporting, not validity rules.

## Ownership

| Area | Location |
|---|---|
| Witness/domain types | `types.rs` |
| Full orchestration | `lib.rs` |
| Match/settlement/orders/block | corresponding verifier modules |
| System/sidecar/quarantine | `system.rs`, `sidecar.rs`, `quarantine.rs` |
| Keys and intent | `account_keys.rs`, `key_transition.rs`, `key_op_auth.rs` |
| Canonical bytes | `canonical.rs`, `witness_schema.rs`, event/state/snapshot schemas |
| Violations/results | `violations.rs` |

## Rules

- Never fork canonical byte layouts between host and guest.
- Preserve exact-keyspace/absence proofs, not inclusion alone.
- Use shared checked integer settlement helpers; no floating point.
- Treat tag/version/layout changes as validity changes: update golden vectors,
  decoders, guest fingerprints/commitments, ADR/current docs, and fresh-genesis
  planning together.
- `sybil-verifier` is the sole trusted solver-output verifier; no parallel
  matching-solver verifier exists.

Run `cargo test -p sybil-verifier`, `just golden-check`, and relevant
`sybil-zk`/OpenVM checks for validity-surface changes.
