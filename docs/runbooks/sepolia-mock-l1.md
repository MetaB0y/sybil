---
tags: [runbook, l1, sepolia, devnet]
status: current
last_verified: 2026-07-15
---

# Unsafe Sepolia mock L1 deployment

This profile gives the private devnet a real Sepolia custody/state-machine
footprint without running a prover. It deliberately does **not** verify proofs.
Use it only for wallet, deposit-indexing, root-submission, withdrawal queue,
finalization, restart, and UI/API integration.

## Safety boundary

The deployment script always creates these contracts together:

- a fresh, valueless, publicly mintable `MintableMockUSDC`;
- two `UnsafeSepoliaMockVerifierAdapter` instances that accept every proof;
- `SybilSettlement` and `SybilVault` wired only to those mock contracts.

The unsafe adapter constructor accepts only chain id `11155111`, and both the
Solidity script and shell wrapper independently check that chain. The wrapper
requires an exact confirmation phrase and refuses to write its manifest until
all receipts, code addresses, unsafe markers, and cross-contract references
have been read back successfully.

This is not validity, proof, DA, escape, or real-funds evidence. Never reuse the
vault with another token, send valuable assets to it, describe its roots as
verified, or promote its addresses into a real-verifier deployment.

## Local drill

Run Anvil with Sepolia's chain id:

```bash
anvil --chain-id 11155111 --port 18547
```

In another shell, use Anvil's published default development key:

```bash
export SEPOLIA_RPC_URL=http://127.0.0.1:18547
export PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
export CONFIRM_UNSAFE_SEPOLIA_MOCK=I_UNDERSTAND_PROOFS_ARE_NOT_VERIFIED
export SYBIL_L1_DEPLOYMENT_MANIFEST=target/sepolia-mock-l1-local.json

just contracts-sepolia-mock-deploy
```

The command must fail on any other chain id. The local manifest is an ignored
artifact and should show `mode = unsafe_sepolia_mock`, both unsafe booleans,
chain id, deployment start block, deployer, and address/transaction-hash pairs.

## Sepolia deployment

Use a dedicated funded testnet key. Load it without placing it in shell history:

```bash
export SEPOLIA_RPC_URL=https://your-sepolia-rpc.example
read -rs PRIVATE_KEY
export PRIVATE_KEY
export CONFIRM_UNSAFE_SEPOLIA_MOCK=I_UNDERSTAND_PROOFS_ARE_NOT_VERIFIED
export SYBIL_L1_DEPLOYMENT_MANIFEST=target/sepolia-mock-l1.json

just contracts-sepolia-mock-deploy
unset PRIVATE_KEY
```

Preserve the manifest as the deployment record. Do not commit a private-key
cache or Foundry's `contracts/cache/` broadcast secrets. Contract addresses and
transaction hashes are non-secret; the RPC URL may contain credentials and is
intentionally omitted from the manifest.

## Indexer configuration

For a remotely shared devnet, use two independently operated Sepolia RPC
providers and the finalized trust mode:

```text
SYBIL_L1_CHAIN_ID=11155111
SYBIL_L1_VAULT=<manifest contracts.vault.address>
SYBIL_L1_START_BLOCK=<manifest deployment_start_block>
SYBIL_L1_TRUST_MODE=unanimous-finalized
SYBIL_L1_RPC_URLS=<provider-a>,<provider-b>
SYBIL_L1_RPC_IDS=<stable-id-a>,<stable-id-b>
```

The mock deployment alone does not complete the product flow. Keep signed L2
withdrawal creation out of the public UI until a relay/proof substitute queues
the corresponding L1 claim and the UI exposes its queued/finalized lifecycle.
Real verifier deployment remains a separate fresh deployment, never an upgrade
claim for this unsafe fixture.
