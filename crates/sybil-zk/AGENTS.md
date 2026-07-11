# `sybil-zk`

Guest-safe state-transition/public-input layer shared by native tests and the
OpenVM guest. It binds the canonical witness, qMDB proofs, DA commitment,
bridge checkpoint, and exact public reveal.

## Read first

- [[ZK Integration Path]], [[Block Witness]], [[State Root Schema]], and
  [[Canonical Serialization]]
- `sybil-verifier/AGENTS.md`

## Invariants

- Guest and native verification must execute the same deterministic integer
  rules; no host-only trust shortcut may cross the guest boundary.
- Recompute header, state/events/witness roots, qMDB proofs, deposits, DA, and
  public-input hash from private input. Do not trust duplicated claimed fields.
- Keep guest-compatible dependencies/features narrow and avoid filesystem,
  network, nondeterminism, or floating point in verification.
- Commitment/domain/public-input changes require golden regeneration, guest
  fingerprint/commit rebuild, adapter repin review, and a fresh-genesis
  decision.

Run `cargo test -p sybil-zk`, `just golden-check`, and the explicit OpenVM guest
checks appropriate to the change.
