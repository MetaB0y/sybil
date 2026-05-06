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

```bash
forge fmt
forge build
forge test
```

From the repository root:

```bash
just contracts-test
just contracts-anvil-unsafe-smoke
```
