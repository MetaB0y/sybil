# `sybil-l1-protocol`

Dependency-light Rust twin of Solidity bridge ABI, event parsing, deposit-tree
hashing, nullifier domains, and public-input word encoding.

## Read first

- [[L1 Settlement and Vault]] and [[Canonical Serialization]]
- `contracts/AGENTS.md`

## Invariants

- Event signatures, indexed/data layout, ABI word padding, domains, tree depth,
  zero hashes, and amount units are protocol bytes.
- Reject malformed topic counts, high bytes, lengths, and overflow; never
  accept a convenient partial decode.
- Keep this crate independent of sequencer/API types. Higher layers convert
  neutral L1 records into exchange operations.
- Any byte change requires Rust/Solidity golden-vector parity.

Run `cargo test -p sybil-l1-protocol` and `forge test` in `contracts/` for
shared-domain changes.
