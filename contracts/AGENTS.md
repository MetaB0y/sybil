# AGENTS.md

This directory contains the Solidity L1 bridge contracts for Sybil.

## Commands

Run from the repository root:

```bash
just contracts-fmt
just contracts-fmt-check
just contracts-build
just contracts-test
just contracts-coverage
just contracts-sepolia-mock-deploy
```

Or from this directory:

```bash
forge fmt
forge build
forge test
```

## Design Context

Read these architecture notes before changing contracts:

- `docs/architecture/04-verification/L1 Settlement and Vault.md`
- `docs/architecture/04-verification/State Root Schema.md`
- `docs/architecture/04-verification/ZK Integration Path.md`
- `docs/architecture/04-verification/Canonical Serialization.md`

## Constraints

- Solidity + Foundry.
- Keep contracts dependency-light until the real OpenVM Solidity SDK is wired.
- Use `IOpenVmVerifierAdapter` as the boundary to OpenVM verifier internals.
- First asset is a USDC-like 6-decimal ERC20.
- Deposit log is a fixed-depth 32 keccak Merkle accumulator with leaf/node domain separation.
- Normal withdrawals use committed withdrawal leaves and root-independent nullifiers.
- Do not implement prediction-market settlement or qmdb proof verification in Solidity.
