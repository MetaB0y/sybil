# Sybil Contracts

Solidity + Foundry project for Sybil's L1 settlement and vault contracts.

The first skeleton uses a mock OpenVM verifier adapter. The real OpenVM
Solidity SDK is intended to plug in behind `IOpenVmVerifierAdapter` without
changing `SybilSettlement` or `SybilVault`.

```bash
forge fmt
forge build
forge test
```

From the repository root:

```bash
just contracts-test
```
