# `contracts`

Solidity settlement/vault boundary. Read [[L1 Settlement and Vault]],
[[State Root Schema]], [[ZK Integration Path]], and [[Canonical Serialization]].

- `IOpenVmVerifierAdapter` isolates OpenVM verifier internals.
- The first collateral is a 6-decimal USDC-like ERC20.
- Deposits use the fixed-depth-32, domain-separated keccak accumulator.
- Normal withdrawals use committed leaves and root-independent nullifiers.
- Prediction-market matching and qMDB verification stay out of Solidity.
- Keep dependencies narrow until the production verifier SDK is deliberately
  integrated.

Run `just contracts-test`; shared bytes/domains also require
`just golden-check` and the owning Rust tests.
