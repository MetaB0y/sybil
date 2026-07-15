#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/scripts/lib/unsafe-sepolia-mock.sh"

for command in cast curl jq; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "missing required command: $command" >&2
        exit 1
    fi
done

: "${SEPOLIA_RPC_URL:?Set SEPOLIA_RPC_URL to the mock deployment RPC}"
: "${PRIVATE_KEY:?Set PRIVATE_KEY to a funded Sepolia transaction key}"
: "${SYBIL_API_URL:?Set SYBIL_API_URL to the sybil-api origin}"
: "${SYBIL_SERVICE_TOKEN:?Set SYBIL_SERVICE_TOKEN for the private relay feed}"
: "${SYBIL_L1_DEPLOYMENT_MANIFEST:?Set SYBIL_L1_DEPLOYMENT_MANIFEST to the validated deployment record}"

if [[ "${CONFIRM_UNSAFE_SEPOLIA_MOCK_RELAY:-}" != "I_UNDERSTAND_WITHDRAWALS_ARE_NOT_PROOF_VERIFIED" ]]; then
    echo "refusing unsafe relay: set CONFIRM_UNSAFE_SEPOLIA_MOCK_RELAY=I_UNDERSTAND_WITHDRAWALS_ARE_NOT_PROOF_VERIFIED" >&2
    exit 1
fi

manifest="$SYBIL_L1_DEPLOYMENT_MANIFEST"
if [[ "$manifest" != /* ]]; then
    manifest="$repo_root/$manifest"
fi
unsafe_sepolia_validate_deployment "$manifest" "$SEPOLIA_RPC_URL"

token="$(jq -er '.contracts.token.address' "$manifest")"
settlement="$(jq -er '.contracts.settlement.address' "$manifest")"
vault="$(jq -er '.contracts.vault.address' "$manifest")"
api_url="${SYBIL_API_URL%/}"

work="$(mktemp -d "${TMPDIR:-/tmp}/sybil-sepolia-relay.XXXXXX")"
trap 'rm -rf "$work"' EXIT

api_get() {
    local path="$1" output="$2" auth="${3:-public}"
    local -a args=(--fail-with-body --silent --show-error --max-time 30
        -H 'Accept: application/json' -o "$output")
    if [[ "$auth" == "service" ]]; then
        args+=(-H "Authorization: Bearer $SYBIL_SERVICE_TOKEN")
    fi
    curl "${args[@]}" "$api_url$path"
}

api_get /v1/bridge/status "$work/status.json"
jq -e --arg vault "${vault,,}" --arg token "${token,,}" '
    .configured_domain.chain_id == 11155111
    and ((.configured_domain.vault_address_hex | ascii_downcase | sub("^0x"; ""))
        == ($vault | sub("^0x"; "")))
    and ((.configured_domain.token_address_hex | ascii_downcase | sub("^0x"; ""))
        == ($token | sub("^0x"; "")))
' "$work/status.json" >/dev/null || {
    echo "sybil-api bridge domain does not match the validated Sepolia mock deployment" >&2
    exit 1
}

api_get /v1/bridge/withdrawals/pending "$work/pending.json" service
jq -e --arg token "${token,,}" '
    def uint: type == "number" and floor == . and . >= 0 and . <= 9007199254740991;
    def hexbytes($n): type == "string" and test("^(0x)?[0-9a-fA-F]+$")
        and ((sub("^0x"; "") | length) == ($n * 2));
    type == "array"
    and all(.[];
        (.withdrawal_id | uint and . >= 1)
        and (.account_id | uint)
        and (.recipient_hex | hexbytes(20))
        and (.token_hex | hexbytes(20))
        and ((.token_hex | ascii_downcase | sub("^0x"; "")) == ($token | sub("^0x"; "")))
        and (.amount_token_units | uint and . >= 1)
        and (.expiry_height | uint)
        and (.created_at_height | uint and . >= 1)
        and (.nullifier_hex | hexbytes(32))
        and .l1_status == "not_requested")
    and (([.[].withdrawal_id] | unique | length) == length)
    and (([.[].nullifier_hex | ascii_downcase] | unique | length) == length)
' "$work/pending.json" >/dev/null || {
    echo "pending withdrawal feed failed shape, uniqueness, or token validation" >&2
    exit 1
}

pending_count="$(jq 'length' "$work/pending.json")"
if [[ "$pending_count" -eq 0 ]]; then
    echo "unsafe Sepolia mock relay: no pending withdrawals"
    exit 0
fi

# The API remains `not_requested` until the confirmed-log indexer observes the
# queue transaction. Check the vault first so a crash/restart in that interval
# does not submit another root or replay the request.
actionable_rows=()
already_queued=0
while IFS= read -r row; do
    withdrawal_id="$(jq -r '.withdrawal_id' <<<"$row")"
    nullifier="0x$(jq -r '.nullifier_hex | sub("^0x"; "")' <<<"$row")"
    if [[ "$(cast call "$vault" 'nullifierUsed(bytes32)(bool)' "$nullifier" \
        --rpc-url "$SEPOLIA_RPC_URL")" == "true" ]]; then
        already_queued=$((already_queued + 1))
        echo "unsafe Sepolia mock relay: withdrawal $withdrawal_id already queued; awaiting indexer"
    else
        actionable_rows+=("$row")
    fi
done < <(jq -c 'sort_by(.withdrawal_id)[]' "$work/pending.json")

if [[ "${#actionable_rows[@]}" -eq 0 ]]; then
    echo "unsafe Sepolia mock relay complete: queued=0 already_queued=$already_queued"
    echo "run sybil-l1-indexer to ingest queue events"
    exit 0
fi
printf '%s\n' "${actionable_rows[@]}" | jq -s '.' >"$work/actionable.json"

l1_block="$(cast block-number --rpc-url "$SEPOLIA_RPC_URL")"
jq -e --argjson l1_block "$l1_block" 'all(.[]; .expiry_height >= $l1_block)' \
    "$work/actionable.json" >/dev/null || {
    echo "refusing batch: at least one pending withdrawal is already expired at L1 block $l1_block" >&2
    exit 1
}

api_get /v1/blocks/latest "$work/block.json"
jq -e '
    def uint: type == "number" and floor == . and . >= 0 and . <= 9007199254740991;
    (.height | uint)
    and (.events_root | test("^[0-9a-fA-F]{64}$"))
    and (.bridge.deposit_root_hex | test("^[0-9a-fA-F]{64}$"))
    and (.bridge.deposit_count | uint)
' "$work/block.json" >/dev/null || {
    echo "latest public block is missing or has malformed settlement fields" >&2
    exit 1
}
api_height="$(jq -er '.height' "$work/block.json")"
max_created_height="$(jq '[.[].created_at_height] | max' "$work/actionable.json")"
if (( api_height < max_created_height )); then
    echo "latest committed API block $api_height does not yet contain every pending withdrawal (need $max_created_height)" >&2
    exit 1
fi
api_get "/v1/da/$api_height/manifest" "$work/da.json"

jq -e --argjson height "$api_height" '
    def uint: type == "number" and floor == . and . >= 0 and . <= 9007199254740991;
    (.height | uint) and .height == $height
    and (.state_root | test("^[0-9a-fA-F]{64}$"))
    and (.block_hash | test("^[0-9a-fA-F]{64}$"))
    and (.witness_root | test("^[0-9a-fA-F]{64}$"))
    and (.da_commitment | test("^[0-9a-fA-F]{64}$"))
' "$work/da.json" >/dev/null || {
    echo "latest DA manifest is missing or malformed" >&2
    exit 1
}

state_root="0x$(jq -er '.state_root' "$work/da.json")"
block_hash="0x$(jq -er '.block_hash' "$work/da.json")"
events_root="0x$(jq -er '.events_root' "$work/block.json")"
witness_root="0x$(jq -er '.witness_root' "$work/da.json")"
da_commitment="0x$(jq -er '.da_commitment' "$work/da.json")"
deposit_root="0x$(jq -er '.bridge.deposit_root_hex' "$work/block.json")"
deposit_count="$(jq -er '.bridge.deposit_count' "$work/block.json")"

vault_deposit_root="$(cast call "$vault" 'depositRootByCount(uint64)(bytes32)' \
    "$deposit_count" --rpc-url "$SEPOLIA_RPC_URL")"
if [[ "${vault_deposit_root,,}" != "${deposit_root,,}" ]]; then
    echo "API deposit checkpoint does not exist in the configured vault" >&2
    exit 1
fi

settlement_height_raw="$(cast call "$settlement" 'latestHeight()(uint64)' \
    --rpc-url "$SEPOLIA_RPC_URL")"
settlement_height="$(cast to-dec "${settlement_height_raw%% *}")"
settlement_root="$(cast call "$settlement" 'latestStateRoot()(bytes32)' \
    --rpc-url "$SEPOLIA_RPC_URL")"

if (( settlement_height > api_height )); then
    echo "settlement head $settlement_height is ahead of API head $api_height; refusing cross-chain state" >&2
    exit 1
elif (( settlement_height == api_height )); then
    if [[ "${settlement_root,,}" != "${state_root,,}" ]]; then
        echo "settlement and API disagree on state root at height $api_height" >&2
        exit 1
    fi
else
    cast send "$settlement" \
        'submitStateRoot((uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64),bytes)' \
        "($settlement_height,$api_height,$settlement_root,$state_root,$block_hash,$events_root,$witness_root,$da_commitment,$deposit_root,$deposit_count)" \
        0x01 --rpc-url "$SEPOLIA_RPC_URL" --private-key "$PRIVATE_KEY" >/dev/null
    accepted_root="$(cast call "$settlement" 'latestStateRoot()(bytes32)' \
        --rpc-url "$SEPOLIA_RPC_URL")"
    if [[ "${accepted_root,,}" != "${state_root,,}" ]]; then
        echo "state-root submission receipt succeeded but settlement head did not advance" >&2
        exit 1
    fi
    echo "unsafe Sepolia mock relay: submitted API root at height $api_height"
fi

claim_kind="$(cast call "$vault" 'CLAIM_KIND_NORMAL()(bytes32)' \
    --rpc-url "$SEPOLIA_RPC_URL")"
queued=0
while IFS= read -r row; do
    withdrawal_id="$(jq -r '.withdrawal_id' <<<"$row")"
    nullifier="0x$(jq -r '.nullifier_hex | sub("^0x"; "")' <<<"$row")"
    recipient="0x$(jq -r '.recipient_hex | sub("^0x"; "")' <<<"$row")"
    amount="$(jq -r '.amount_token_units' <<<"$row")"

    if [[ "$(cast call "$vault" 'nullifierUsed(bytes32)(bool)' "$nullifier" \
        --rpc-url "$SEPOLIA_RPC_URL")" == "true" ]]; then
        already_queued=$((already_queued + 1))
        echo "unsafe Sepolia mock relay: withdrawal $withdrawal_id already queued; awaiting indexer"
        continue
    fi

    cast send "$vault" \
        'requestWithdrawal((bytes32,uint64,bytes32,address,address,uint256,bytes32),bytes)' \
        "($state_root,$api_height,$nullifier,$recipient,$token,$amount,$claim_kind)" \
        0x01 --rpc-url "$SEPOLIA_RPC_URL" --private-key "$PRIVATE_KEY" >/dev/null
    queued=$((queued + 1))
    echo "unsafe Sepolia mock relay: queued withdrawal $withdrawal_id"
done < <(jq -c 'sort_by(.withdrawal_id)[]' "$work/actionable.json")

echo "unsafe Sepolia mock relay complete: queued=$queued already_queued=$already_queued"
echo "run sybil-l1-indexer to ingest queue events; finalization remains a separate delayed action"
