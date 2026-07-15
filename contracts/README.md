# Sybil Contracts

Solidity + Foundry project for Sybil's L1 settlement and vault contracts.

`SybilSettlement` and `SybilVault` depend only on `IOpenVmVerifierAdapter`.
Tests use `MockOpenVmVerifierAdapter`; production deployments use
`OpenVmVerifierAdapter`, which wraps the generated OpenVM Halo2 verifier,
pins the Sybil guest executable/VM commitments, and checks the revealed public
input hash before accepting a proof.

Local Anvil/devnet plumbing can use
`src/dev/UnsafeAcceptAllVerifierAdapter.sol`. It deliberately accepts every
proof while preserving the same `IOpenVmVerifierAdapter` boundary. Do not use
that adapter in any production or public testnet deployment.

The separate `UnsafeSepoliaMockVerifierAdapter` is constructor-bound to Sepolia
and may be used only by the documented mock profile with a freshly deployed
publicly mintable `MintableMockUSDC`. It is public-testnet plumbing, not proof
verification or a real-funds deployment.

```bash
forge fmt
forge build
forge test
```

Money-path branch coverage is a separate filtered gate. It excludes tests,
mocks, deployment scripts, and the unsafe development adapter, then enforces
per-contract plus aggregate floors documented in [`COVERAGE.md`](COVERAGE.md).

From the repository root:

```bash
just contracts-test
just contracts-coverage
just contracts-sepolia-mock-deploy
just contracts-anvil-unsafe-smoke
```
