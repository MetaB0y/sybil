# `sybil-custody`

Distrustful user-side tooling for own-leaf retention, full DA reconstruction,
and Form-L escape proving/submission. The binary is a thin shell over testable
library modules.

## Read first

- [[Operator Replacement]], [[L1 Settlement and Vault]], and [[Data Availability]]
- `README.md` and `sybil-escape-claim/AGENTS.md`

## Boundaries

- Never call an own-leaf snapshot a full exchange backup. Reconstruction needs
  the canonical DA payload and its complete binding chain.
- Authenticate height/root/manifest against the intended L1 settlement before
  trusting a payload or claim amount.
- The P256 scalar authorizes the Sybil escape statement; the Ethereum
  transaction key is separate.
- Fixture proofs are unsafe Anvil-only plumbing. Real proving must remain an
  explicit, fail-closed path.
- Preserve versioned JSON formats and deterministic ABI bytes.

Run `cargo test -p sybil-custody`; the real proving drill remains explicitly
gated because it requires a proving-capable machine.
