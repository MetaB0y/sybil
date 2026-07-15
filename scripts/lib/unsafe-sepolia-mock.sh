#!/usr/bin/env bash

# Shared read-only validation for the explicitly unsafe Sepolia mock
# deployment. Callers remain responsible for confirmation phrases and any
# state-changing transaction.

unsafe_sepolia_assert_address_call() {
    local rpc_url="$1" contract="$2" signature="$3" expected="$4"
    local actual
    actual="$(cast call "$contract" "$signature" --rpc-url "$rpc_url")"
    if [[ "${actual,,}" != "${expected,,}" ]]; then
        echo "$contract $signature returned $actual; expected $expected" >&2
        return 1
    fi
}

unsafe_sepolia_validate_deployment() {
    local manifest="$1" rpc_url="$2"

    if [[ ! -f "$manifest" ]]; then
        echo "missing unsafe Sepolia deployment manifest: $manifest" >&2
        return 1
    fi
    if [[ "$(cast chain-id --rpc-url "$rpc_url")" != "11155111" ]]; then
        echo "unsafe Sepolia mock operation requires chain 11155111" >&2
        return 1
    fi

    jq -e '
        def address: type == "string" and test("^0x[0-9a-fA-F]{40}$");
        def tx_hash: type == "string" and test("^0x[0-9a-fA-F]{64}$");
        .schema_version == 1
        and .mode == "unsafe_sepolia_mock"
        and .chain_id == 11155111
        and .unsafe_accepts_all_proofs == true
        and .collateral_is_publicly_mintable == true
        and (.deployment_start_block | type == "number" and . >= 0)
        and (.deployer | address)
        and all(.contracts[]; (.address | address) and (.transaction_hash | tx_hash))
        and ([.contracts | keys[]] | sort ==
            ["escape_verifier", "settlement", "token", "vault", "verifier"])
    ' "$manifest" >/dev/null || {
        echo "unsafe Sepolia deployment manifest failed schema/trust-boundary validation" >&2
        return 1
    }

    local token verifier escape_verifier settlement vault address
    token="$(jq -er '.contracts.token.address' "$manifest")"
    verifier="$(jq -er '.contracts.verifier.address' "$manifest")"
    escape_verifier="$(jq -er '.contracts.escape_verifier.address' "$manifest")"
    settlement="$(jq -er '.contracts.settlement.address' "$manifest")"
    vault="$(jq -er '.contracts.vault.address' "$manifest")"

    for address in "$token" "$verifier" "$escape_verifier" "$settlement" "$vault"; do
        if [[ "$(cast code "$address" --rpc-url "$rpc_url")" == "0x" ]]; then
            echo "unsafe Sepolia deployment address has no code: $address" >&2
            return 1
        fi
    done

    unsafe_sepolia_assert_address_call "$rpc_url" "$settlement" 'vault()(address)' "$vault"
    unsafe_sepolia_assert_address_call "$rpc_url" "$settlement" 'verifier()(address)' "$verifier"
    unsafe_sepolia_assert_address_call "$rpc_url" "$vault" 'token()(address)' "$token"
    unsafe_sepolia_assert_address_call "$rpc_url" "$vault" 'settlement()(address)' "$settlement"
    unsafe_sepolia_assert_address_call "$rpc_url" "$vault" 'verifier()(address)' "$verifier"
    unsafe_sepolia_assert_address_call \
        "$rpc_url" "$vault" 'escapeVerifier()(address)' "$escape_verifier"

    if [[ "$(cast call "$verifier" 'unsafeAcceptsAllProofs()(bool)' --rpc-url "$rpc_url")" != "true" ]]; then
        echo "normal verifier is missing the unsafe accept-all marker" >&2
        return 1
    fi
    if [[ "$(cast call "$escape_verifier" 'unsafeAcceptsAllProofs()(bool)' --rpc-url "$rpc_url")" != "true" ]]; then
        echo "escape verifier is missing the unsafe accept-all marker" >&2
        return 1
    fi
}
