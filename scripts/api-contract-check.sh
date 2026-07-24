#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
allowlist="$repo_root/scripts/api-contract-allowlist.json"
validator="$repo_root/scripts/check-api-contract-allowlist.py"
schemathesis_version="4.23.0"
contract_root=""
server_pid=""

cleanup() {
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    if [[ -n "$contract_root" && "$contract_root" == "${TMPDIR:-/tmp}"/sybil-api-contract.* ]]; then
        rm -rf -- "$contract_root"
    fi
}
trap cleanup EXIT

for command in cargo curl python3 uvx; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "error: api-contract-check requires $command" >&2
        exit 1
    }
done

binary="${SYBIL_CONTRACT_BINARY:-}"
if [[ -z "$binary" ]]; then
    cargo build \
        --manifest-path "$repo_root/Cargo.toml" \
        -p sybil-api \
        --bin sybil-api
    target_dir="$(
        cargo metadata \
            --manifest-path "$repo_root/Cargo.toml" \
            --format-version 1 \
            --no-deps |
            python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
    )"
    binary="$target_dir/debug/sybil-api"
elif [[ "$binary" != /* ]]; then
    binary="$repo_root/$binary"
fi
[[ -x "$binary" ]] || {
    echo "error: Sybil API binary is not executable: $binary" >&2
    exit 1
}

contract_root="$(mktemp -d "${TMPDIR:-/tmp}/sybil-api-contract.XXXXXX")"
contract_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
base_url="http://127.0.0.1:$contract_port"
openapi_url="$base_url/openapi.json"

RUST_LOG=error \
SYBIL_HTTP_ONBOARDING_GLOBAL_RPS=100000 \
SYBIL_HTTP_ONBOARDING_GLOBAL_BURST=100000 \
SYBIL_HTTP_ONBOARDING_CLIENT_RPS=100000 \
SYBIL_HTTP_ONBOARDING_CLIENT_BURST=100000 \
"$binary" \
    --dev-mode \
    --port "$contract_port" \
    --data-dir "$contract_root/data" \
    --admin-feed-key-path "$contract_root/admin-feed.key" \
    --block-interval-ms 60000 \
    >"$contract_root/server.log" 2>&1 &
server_pid=$!

healthy=0
for _attempt in $(seq 1 240); do
    if curl --fail --silent --show-error "$base_url/v1/health" >/dev/null 2>&1; then
        healthy=1
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        break
    fi
    sleep 0.25
done
if [[ "$healthy" != 1 ]]; then
    echo "error: disposable Sybil API did not become healthy" >&2
    tail -100 "$contract_root/server.log" >&2
    exit 1
fi

python3 "$validator" --openapi "$openapi_url" --allowlist "$allowlist"
mapfile -t positive_exclusions < <(
    python3 "$validator" --openapi "$openapi_url" --allowlist "$allowlist" --phase positive
)
mapfile -t negative_exclusions < <(
    python3 "$validator" --openapi "$openapi_url" --allowlist "$allowlist" --phase negative
)

positive_args=()
for operation_id in "${positive_exclusions[@]}"; do
    positive_args+=(--exclude-operation-id "$operation_id")
done
negative_args=()
for operation_id in "${negative_exclusions[@]}"; do
    negative_args+=(--exclude-operation-id "$operation_id")
done

common_args=(
    --phases fuzzing
    --max-examples 5
    --generation-deterministic
    --workers 1
    --no-shrink
    --max-failures 20
    --suppress-health-check=filter_too_much
    --no-color
)

(
    cd "$contract_root"
    uvx --from "schemathesis==$schemathesis_version" schemathesis run "$openapi_url" \
        --checks status_code_conformance,content_type_conformance,response_schema_conformance,positive_data_acceptance \
        --mode positive \
        "${common_args[@]}" \
        "${positive_args[@]}"
)
(
    cd "$contract_root"
    uvx --from "schemathesis==$schemathesis_version" schemathesis run "$openapi_url" \
        --checks status_code_conformance,content_type_conformance,response_schema_conformance,negative_data_rejection \
        --mode negative \
        "${common_args[@]}" \
        "${negative_args[@]}"
)

echo "API contract check passed with Schemathesis $schemathesis_version"
