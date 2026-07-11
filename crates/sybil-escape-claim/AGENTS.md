# `sybil-escape-claim`

Guest-safe verifier for conservative Form-L escape claims. It authenticates
the account/reservation/market openings, active key, valuation, nullifier, and
exact public statement revealed to L1.

## Read first

- [[L1 Settlement and Vault]], [[State Root Schema]], and [[P256 Authentication]]
- [ADR-0013](../../docs/adr/0013-exit-and-escape-model.md)

## Invariants

- No floating point. Valuation uses committed last-clearing prices and checked
  integer notional helpers.
- MINT cannot claim. Missing reservation is an authenticated exclusion, not an
  assumed zero.
- Market proofs must be complete, unique, and exactly relevant to positions.
- Active keys must match `keys_digest`; authorization, deployment binding,
  amount, and nullifier all fail closed.
- Rust public-input/nullifier domains must stay byte-identical to Solidity and
  golden vectors.

Run `cargo test -p sybil-escape-claim` plus contract golden-vector tests when
domains or public inputs change.
