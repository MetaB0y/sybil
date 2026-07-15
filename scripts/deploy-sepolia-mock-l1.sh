#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/scripts/lib/unsafe-sepolia-mock.sh"

for command in forge cast jq; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "missing required command: $command" >&2
        exit 1
    fi
done

: "${SEPOLIA_RPC_URL:?Set SEPOLIA_RPC_URL to a Sepolia JSON-RPC endpoint}"
: "${PRIVATE_KEY:?Set PRIVATE_KEY to the funded Sepolia deployer key}"

if [[ "${CONFIRM_UNSAFE_SEPOLIA_MOCK:-}" != "I_UNDERSTAND_PROOFS_ARE_NOT_VERIFIED" ]]; then
    echo "refusing unsafe deployment: set CONFIRM_UNSAFE_SEPOLIA_MOCK=I_UNDERSTAND_PROOFS_ARE_NOT_VERIFIED" >&2
    exit 1
fi

chain_id="$(cast chain-id --rpc-url "$SEPOLIA_RPC_URL")"
if [[ "$chain_id" != "11155111" ]]; then
    echo "refusing unsafe deployment on chain $chain_id; expected Sepolia 11155111" >&2
    exit 1
fi

(
    cd "$repo_root/contracts"
    forge script script/UnsafeSepoliaMockSetup.s.sol:UnsafeSepoliaMockSetup \
        --rpc-url "$SEPOLIA_RPC_URL" \
        --broadcast
)

broadcast="$repo_root/contracts/broadcast/UnsafeSepoliaMockSetup.s.sol/11155111/run-latest.json"
if [[ ! -f "$broadcast" ]]; then
    echo "missing Foundry broadcast artifact: $broadcast" >&2
    exit 1
fi

manifest_path="${SYBIL_L1_DEPLOYMENT_MANIFEST:-target/sepolia-mock-l1.json}"
if [[ "$manifest_path" != /* ]]; then
    manifest_path="$repo_root/$manifest_path"
fi
mkdir -p "$(dirname "$manifest_path")"
manifest_tmp="$(mktemp "${manifest_path}.tmp.XXXXXX")"
trap 'rm -f "$manifest_tmp"' EXIT

deployment_start_block_hex="$(jq -er '.receipts[0].blockNumber' "$broadcast")"
deployment_start_block="$(cast to-dec "$deployment_start_block_hex")"

jq -e --argjson deployment_start_block "$deployment_start_block" '
    def artifact($transaction): {
        address: $transaction.contractAddress,
        transaction_hash: $transaction.hash
    };

    [.transactions[] | select(.transactionType == "CREATE")] as $creates
    | [$creates[] | select(.contractName == "MintableMockUSDC")] as $tokens
    | [$creates[] | select(.contractName == "UnsafeSepoliaMockVerifierAdapter")] as $adapters
    | [$creates[] | select(.contractName == "SybilSettlement")] as $settlements
    | [$creates[] | select(.contractName == "SybilVault")] as $vaults
    | if .chain != 11155111 then error("broadcast chain is not Sepolia")
      elif ([.receipts[] | select(.status != "0x1")] | length) != 0
        then error("broadcast contains an unsuccessful receipt")
      elif ($tokens | length) != 1 or ($adapters | length) != 2
        or ($settlements | length) != 1 or ($vaults | length) != 1
        then error("broadcast does not contain the expected deployment shape")
      else {
        schema_version: 1,
        mode: "unsafe_sepolia_mock",
        chain_id: .chain,
        deployment_start_block: $deployment_start_block,
        broadcast_timestamp_ms: .timestamp,
        deployer: .transactions[0].transaction.from,
        unsafe_accepts_all_proofs: true,
        collateral_is_publicly_mintable: true,
        contracts: {
            token: artifact($tokens[0]),
            verifier: artifact($adapters[0]),
            escape_verifier: artifact($adapters[1]),
            settlement: artifact($settlements[0]),
            vault: artifact($vaults[0])
        }
      }
      end
' "$broadcast" >"$manifest_tmp"

unsafe_sepolia_validate_deployment "$manifest_tmp" "$SEPOLIA_RPC_URL"

mv "$manifest_tmp" "$manifest_path"
trap - EXIT

echo "unsafe Sepolia mock deployment validated"
echo "manifest: $manifest_path"
jq '.contracts | map_values(.address)' "$manifest_path"
