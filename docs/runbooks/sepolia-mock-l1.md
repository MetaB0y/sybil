---
tags: [runbook, l1, sepolia, devnet]
status: current
last_verified: 2026-07-18
---

# Unsafe Sepolia mock L1 deployment

This profile gives prelaunch a real Sepolia custody/state-machine
footprint without running a prover. It deliberately does **not** verify proofs.
Use it only for wallet, deposit-indexing, root-submission, withdrawal queue,
finalization, restart, and UI/API integration. The one-shot relay is
restart-safe across the queue/indexer gap, but it is not a proof system.

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

The API has a separate all-or-none admission domain. Set it from the same
validated manifest before accepting either deposits or withdrawals:

```text
SYBIL_BRIDGE_CHAIN_ID=11155111
SYBIL_BRIDGE_VAULT_ADDRESS=<manifest contracts.vault.address>
SYBIL_BRIDGE_TOKEN_ADDRESS=<manifest contracts.token.address>
```

`GET /v1/bridge/status` exposes that configured domain. With any field missing,
deposit and withdrawal creation return `503 BRIDGE_UNAVAILABLE`; a request for
another chain, vault, or token returns `400 BRIDGE_DOMAIN_MISMATCH` before the
sequencer mutates balance or its acknowledged-write log. This is an honest
operator guard, not a guest-proven invariant; [GitHub #92](https://github.com/MetaB0y/sybil/issues/92)
tracks the validity-level domain binding required before real funds.

## Unsafe withdrawal relay

The relay reads only the service-authenticated
`GET /v1/bridge/withdrawals/pending` feed. On every run it:

1. revalidates chain `11155111`, manifest shape, deployed code, contract
   cross-wiring, mintable-collateral marker, and both accept-all markers;
2. requires the API's configured chain/vault/token to equal that manifest;
3. rejects malformed, duplicate, wrong-token, or already-expired leaves;
4. submits at most one newer committed API root after matching the exact vault
   deposit checkpoint; and
5. queues each unused nullifier while skipping rows already consumed on L1.

The final property makes a retry safe when the first process queued a claim but
crashed before `sybil-l1-indexer` advanced API status. A retry does not submit
another root or request that nullifier again.

Load the same non-secret manifest and a funded testnet transaction key:

```bash
export SYBIL_API_URL=https://your-devnet.example
export SYBIL_SERVICE_TOKEN=...
export SEPOLIA_RPC_URL=https://your-sepolia-rpc.example
read -rs PRIVATE_KEY
export PRIVATE_KEY
export SYBIL_L1_DEPLOYMENT_MANIFEST=target/sepolia-mock-l1.json
export CONFIRM_UNSAFE_SEPOLIA_MOCK_RELAY=I_UNDERSTAND_WITHDRAWALS_ARE_NOT_PROOF_VERIFIED

just contracts-sepolia-mock-relay
unset PRIVATE_KEY
```

Run the indexer after the relay so `WithdrawalQueued` becomes visible through
the account-scoped status API. The relay deliberately does not finalize:
Sepolia's mock vault has a one-hour withdrawal delay, and anyone may call
`finalizeWithdrawal(nullifier)` after `l1_executable_at_unix`. Run the indexer
again after finalization to ingest the terminal event.

Keep signed L2 withdrawal creation out of the public UI until the wallet flow
can submit an authorized request, show relay/indexer health, and expose the
queued/finalizable/finalized lifecycle without implying validity. Real verifier
deployment remains a separate fresh deployment, never an upgrade claim for
this unsafe fixture.

## Local end-to-end drill

`scripts/itest-compose.sh` now starts Anvil as chain `11155111`,
deploys this exact mock profile before API boot, injects the validated admission
domain, indexes a deposit, creates a signed withdrawal, runs the relay twice to
prove retry idempotence, advances the one-hour delay, finalizes, and indexes the
terminal status. The default performs no proving; `--with-escape` is a separate
explicit opt-in for the older custody drill.
