#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/lib/smoke-common.sh

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

assert_eq() {
    local actual=$1 expected=$2 label=$3
    [[ "$actual" == "$expected" ]] || fail "$label: expected '$expected', got '$actual'"
}

assert_proof_result() {
    local body=$1 chain=$2 limit=$3 canonical_root=$4 expected_output=$5 expected_status=$6
    local output status
    set +e
    output=$(printf '%s' "$body" | smoke_proof_lag_result "$chain" "$limit" "$canonical_root")
    status=$?
    set -e
    assert_eq "$output" "$expected_output" "proof lag output"
    assert_eq "$status" "$expected_status" "proof lag status"
}

inventory=$(printf '%s' '[
  {"market_id": 7, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"},
  {"market_id": 8, "status": "active", "polymarket_condition_id": "0xabc", "reference_price_nanos": "500000000"},
  {"market_id": 9, "status": "active", "polymarket_condition_id": "0xdef", "reference_price_nanos": null}
]' | smoke_market_inventory)
assert_eq "$inventory" "OK 1 2 1 7" "market inventory"
read -r status native mirrored referenced _ <<< "$inventory"
smoke_market_inventory_is_ready "$status" "$native" "$mirrored" "$referenced" \
    || fail "ready market inventory was rejected"

unready=$(printf '%s' '[
  {"market_id": 3, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"}
]' | smoke_market_inventory)
assert_eq "$unready" "OK 1 0 0 3" "unready mirror inventory"
read -r status native mirrored referenced _ <<< "$unready"
if smoke_market_inventory_is_ready "$status" "$native" "$mirrored" "$referenced"; then
    fail "zero-mirror inventory was ready"
fi

no_reference=$(printf '%s' '[
  {"market_id": 3, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"},
  {"market_id": 4, "status": "active", "polymarket_condition_id": "0xabc", "reference_price_nanos": null}
]' | smoke_market_inventory)
assert_eq "$no_reference" "OK 1 1 0 3" "unreferenced mirror inventory"
read -r status native mirrored referenced _ <<< "$no_reference"
if smoke_market_inventory_is_ready "$status" "$native" "$mirrored" "$referenced"; then
    fail "unreferenced mirror inventory was ready"
fi

zero_and_inactive=$(printf '%s' '[
  {"market_id": 3, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"},
  {"market_id": 4, "status": "active", "polymarket_condition_id": "0xzero", "reference_price_nanos": "0"},
  {"market_id": 5, "status": "resolved", "polymarket_condition_id": "0xold", "reference_price_nanos": "500000000"},
  {"market_id": 6, "status": "active", "polymarket_condition_id": "", "reference_price_nanos": "500000000"}
]' | smoke_market_inventory)
assert_eq "$zero_and_inactive" "OK 1 1 0 3" "zero/inactive mirror inventory"

metrics='# HELP sybil_reference_prices_age_seconds age
# TYPE sybil_reference_prices_age_seconds gauge
sybil_reference_prices_age_seconds 3.824'
age=$(printf '%s\n' "$metrics" | smoke_prometheus_scalar sybil_reference_prices_age_seconds)
assert_eq "$age" "3.824" "reference age metric"
smoke_reference_age_is_fresh "$age" 180 || fail "fresh reference age was rejected"
if smoke_reference_age_is_fresh 180.001 180; then
    fail "stale reference age was accepted"
fi
if smoke_reference_age_is_fresh NaN 180; then
    fail "non-finite reference age was accepted"
fi

if printf '%s' '{"markets": []}' | smoke_market_inventory >/dev/null 2>&1; then
    fail "non-array market inventory was accepted"
fi
if printf '%s' 'not json' | smoke_market_inventory >/dev/null 2>&1; then
    fail "malformed market inventory was accepted"
fi

root_a="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
root_b="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
proof_ready='{"block_height":98,"state_root":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"prepared","proof_status":"mock_verified"}'
proof_stale='{"block_height":90,"state_root":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"prepared","proof_status":"mock_verified"}'
proof_future='{"block_height":105,"state_root":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"prepared","proof_status":"mock_verified"}'
assert_proof_result "$proof_ready" 100 5 "$root_a" "OK 98 2" 0
assert_proof_result "$proof_stale" 100 5 "$root_a" "STALE 90 10" 1
assert_proof_result "$proof_future" 100 5 "$root_a" "ERR future-block-height" 1
assert_proof_result "$proof_ready" 100 5 "$root_b" "ERR state-root-mismatch" 1
assert_proof_result '{"block_height":98,"state_root":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"failed","proof_status":"mock_verified"}' 100 5 "$root_a" "ERR invalid-worker-status" 1
assert_proof_result '{"block_height":98,"state_root":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"prepared","proof_status":"not_started"}' 100 5 "$root_a" "ERR invalid-proof-status" 1
assert_proof_result '{"block_height":"98"}' 100 5 "$root_a" "ERR invalid-block-height" 1
assert_proof_result '{"error":"empty"}' 100 5 "$root_a" "ERR invalid-block-height" 1
assert_proof_result 'not json' 100 5 "$root_a" "ERR malformed-json" 1

# Long-lived services must be running/healthy. The native catalog admin is the
# sole explicit one-shot and passes only after exit 0; arbitrary stopped
# services and failed catalog installs remain fatal.
service_passes=()
service_failures=()
service_skips=()
record_service_pass() { service_passes+=("$1"); }
record_service_failure() { service_failures+=("$1"); }
record_service_skip() { service_skips+=("$1"); }
smoke_docker_available() { return 0; }
smoke_compose_service_rows() {
    printf '%s\n' \
        'sybil-api /stack-sybil-api-1 running healthy 0' \
        'sybil-native-admin /stack-sybil-native-admin-1 exited none 0' \
        'arbitrary-job /stack-arbitrary-job-1 exited none 0' \
        'sybil-native-admin /stack-failed-native-admin-1 exited none 1'
}
smoke_check_compose_services "" stack \
    record_service_pass record_service_failure record_service_skip
assert_eq "${#service_passes[@]}" "2" "compose service pass count"
assert_eq "${#service_failures[@]}" "2" "compose service failure count"
assert_eq "${#service_skips[@]}" "0" "compose service skip count"
[[ "${service_passes[1]}" == *'completed successfully (exit 0)'* ]] \
    || fail "native admin exit 0 was not reported as successful completion"
[[ "${service_failures[0]}" == *'stack-arbitrary-job-1'* ]] \
    || fail "arbitrary exit 0 service did not fail closed"
[[ "${service_failures[1]}" == *'stack-failed-native-admin-1'* ]] \
    || fail "native admin nonzero exit did not fail closed"

echo "smoke-common tests: ok"
