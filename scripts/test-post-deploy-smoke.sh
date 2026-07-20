#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/post-deploy-smoke.sh

if grep -q 'SYBIL_SMOKE_SOURCE_ONLY' scripts/post-deploy-smoke.sh; then
    echo "FAIL: environment-controlled source bypass can disable the deploy gate" >&2
    exit 1
fi

fixture_ready='[
  {"market_id": 7, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"},
  {"market_id": 8, "status": "active", "polymarket_condition_id": "0xabc", "reference_price_nanos": "500000000"}
]'
fixture_unready='[
  {"market_id": 7, "status": "active", "polymarket_condition_id": null, "resolution_criteria": "native"}
]'
metrics_fresh='sybil_reference_prices_age_seconds 2.5'
proof_root="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

reset_gate() {
    PASSN=0; FAILN=0; SKIPN=0; RESULTS=(); ORDER_MARKET=""
    HISTORY_ACCOUNT=""
    VIRTUAL_SLEEP_CALLS=0
    SECONDS=0
}

sleep() {
    VIRTUAL_SLEEP_CALLS=$((VIRTUAL_SLEEP_CALLS + 1))
    SECONDS=$((SECONDS + $1))
}

# Public onboarding must accept the key-only route while stock remains and
# treat durable-cap exhaustion as an expected, explicitly coded policy state.
mint_p256_pub() { printf '%s\n' "04$(printf '11%.0s' {1..64})"; }
for onboarding_enabled in true false; do
    reset_gate
    http() {
        local method=$1 path=$2 body=${3:-}
        if [[ "$method|$path" == "GET|/v1/onboarding" ]]; then
            HTTP_CODE=200
            if [[ "$onboarding_enabled" == true ]]; then
                HTTP_BODY='{"enabled":true,"account_capacity":1000,"accounts_allocated":10,"accounts_remaining":990,"grant_nanos":1000000000000}'
            else
                HTTP_BODY='{"enabled":false,"account_capacity":1000,"accounts_allocated":1000,"accounts_remaining":0,"grant_nanos":1000000000000}'
            fi
        elif [[ "$path" == "/v1/onboarding/accounts" && "$body" == *initial_balance_nanos* ]]; then
            HTTP_CODE=422; HTTP_BODY='{"error":"unknown field"}'
        elif [[ "$path" == "/v1/onboarding/accounts" && "$onboarding_enabled" == true ]]; then
            HTTP_CODE=200; HTTP_BODY='{"account_id":10,"balance_nanos":1000000000000}'
        elif [[ "$path" == "/v1/onboarding/accounts" ]]; then
            HTTP_CODE=409; HTTP_BODY='{"code":"PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED"}'
        else
            HTTP_CODE=401; HTTP_BODY='{"code":"UNAUTHORIZED"}'
        fi
    }
    check_onboarding >/dev/null
    [[ "$FAILN" -eq 0 ]] \
        || { echo "FAIL: onboarding policy state enabled=$onboarding_enabled failed deploy-gate checks" >&2; exit 1; }
done

# A successful allocation with a zero/mismatched balance must block promotion
# even when policy discovery advertised the intended fixed grant.
reset_gate
http() {
    local method=$1 path=$2 body=${3:-}
    if [[ "$method|$path" == "GET|/v1/onboarding" ]]; then
        HTTP_CODE=200
        HTTP_BODY='{"enabled":true,"account_capacity":1000,"accounts_allocated":10,"accounts_remaining":990,"grant_nanos":1000000000000}'
    elif [[ "$path" == "/v1/onboarding/accounts" && "$body" == *initial_balance_nanos* ]]; then
        HTTP_CODE=422; HTTP_BODY='{"error":"unknown field"}'
    elif [[ "$path" == "/v1/onboarding/accounts" ]]; then
        HTTP_CODE=200; HTTP_BODY='{"account_id":10,"balance_nanos":0}'
    else
        HTTP_CODE=401; HTTP_BODY='{"code":"UNAUTHORIZED"}'
    fi
}
check_onboarding >/dev/null
[[ "$FAILN" -eq 1 ]] \
    || { echo "FAIL: onboarding balance mismatch did not block promotion" >&2; exit 1; }

# One unready registry followed by a ready registry + fresh feed proves bounded
# retries recover without weakening the final assertion.
reset_gate
MIRROR_TIMEOUT=10
MIRROR_POLL=2
MIRROR_MAX_AGE=180
SKIP_MIRROR_READINESS=0
market_calls=0
http() {
    local _method=$1 path=$2
    HTTP_CODE=200
    if [[ "$path" == "/v1/markets" ]]; then
        market_calls=$((market_calls + 1))
        if [[ "$market_calls" -eq 1 ]]; then HTTP_BODY=$fixture_unready
        else HTTP_BODY=$fixture_ready; fi
    else
        HTTP_BODY=$metrics_fresh
    fi
}
check_markets >/dev/null
[[ "$FAILN" -eq 0 && "$market_calls" -eq 2 && "$ORDER_MARKET" == "7" ]] \
    || { echo "FAIL: transient mirror readiness did not recover" >&2; exit 1; }

# An unready registry must stop at the deadline and fail, not spin forever or
# sleep beyond the configured boundary.
reset_gate
MIRROR_TIMEOUT=2
MIRROR_POLL=2
SKIP_MIRROR_READINESS=0
market_calls=0
http() {
    market_calls=$((market_calls + 1))
    HTTP_CODE=200
    HTTP_BODY=$fixture_unready
}
check_markets >/dev/null
[[ "$FAILN" -gt 0 && "$market_calls" -eq 1 && "$SECONDS" -eq 2 ]] \
    || { echo "FAIL: mirror timeout boundary was not enforced" >&2; exit 1; }

# Web-only promotion still validates the local native registry immediately but
# does not wait on or fail for the unrelated external integration.
reset_gate
MIRROR_TIMEOUT=180
MIRROR_POLL=5
SKIP_MIRROR_READINESS=1
market_calls=0
http() {
    market_calls=$((market_calls + 1))
    HTTP_CODE=200
    HTTP_BODY=$fixture_unready
}
check_markets >/dev/null
[[ "$FAILN" -eq 0 && "$SKIPN" -eq 1 && "$market_calls" -eq 1 && "$SECONDS" -eq 0 ]] \
    || { echo "FAIL: web-only mirror skip was not isolated" >&2; exit 1; }

# Proof freshness recovers when the mock prover catches up within the bounded
# window, and reports the authoritative chain/proof lag.
reset_gate
PROOF_TIMEOUT=10
PROOF_POLL=2
PROOF_LAG_MAX=5
REQUIRE_PROOF_FRESHNESS=1
smoke_docker_available() { return 0; }
http() {
    local _method=$1 path=$2
    HTTP_CODE=200
    if [[ "$path" == "/v1/blocks/latest" ]]; then HTTP_BODY='{"height":100}'
    else HTTP_BODY="{\"state_root\":\"$proof_root\"}"; fi
}
smoke_compose_service_curl() {
    if [[ "$VIRTUAL_SLEEP_CALLS" -eq 0 ]]; then
        printf '%s' "{\"block_height\":90,\"state_root\":\"0x$proof_root\",\"status\":\"prepared\",\"proof_status\":\"mock_verified\"}"
    else
        printf '%s' "{\"block_height\":98,\"state_root\":\"0x$proof_root\",\"status\":\"prepared\",\"proof_status\":\"mock_verified\"}"
    fi
}
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 0 && "$PASSN" -eq 1 && "$VIRTUAL_SLEEP_CALLS" -eq 1 ]] \
    || { echo "FAIL: transient proof lag did not recover" >&2; exit 1; }

# A permanently stale proof head stops exactly at the deadline and blocks a
# required promotion.
reset_gate
PROOF_TIMEOUT=2
PROOF_POLL=2
PROOF_LAG_MAX=5
REQUIRE_PROOF_FRESHNESS=1
smoke_compose_service_curl() {
    printf '%s' "{\"block_height\":90,\"state_root\":\"0x$proof_root\",\"status\":\"prepared\",\"proof_status\":\"mock_verified\"}"
}
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 1 && "$SECONDS" -eq 2 ]] \
    || { echo "FAIL: stale proof timeout did not block promotion" >&2; exit 1; }

# A status from another genesis at the same height cannot pass by lag alone.
reset_gate
PROOF_TIMEOUT=1
PROOF_LAG_MAX=5
REQUIRE_PROOF_FRESHNESS=1
smoke_compose_service_curl() {
    printf '%s' '{"block_height":98,"state_root":"0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","status":"prepared","proof_status":"mock_verified"}'
}
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 1 ]] \
    || { echo "FAIL: mismatched proof state root did not block promotion" >&2; exit 1; }

# Consuming the whole deadline in the prover/SSH leg must prevent a second
# public-chain request from starting with a fresh timeout budget.
reset_gate
PROOF_TIMEOUT=1
PROOF_POLL=1
REQUIRE_PROOF_FRESHNESS=1
http_calls=0
http() {
    http_calls=$((http_calls + 1))
    HTTP_CODE=200
    HTTP_BODY='{"height":100}'
}
smoke_compose_service_curl() {
    /bin/sleep 1.1
    printf '%s' "{\"block_height\":98,\"state_root\":\"0x$proof_root\",\"status\":\"prepared\",\"proof_status\":\"mock_verified\"}"
}
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 1 && "$http_calls" -eq 0 ]] \
    || { echo "FAIL: proof timeout budget was reused across network legs" >&2; exit 1; }

# An optional product deployment with no running validity service must skip
# immediately rather than spend the full prover catch-up timeout.
reset_gate
PROOF_TIMEOUT=120
REQUIRE_PROOF_FRESHNESS=0
smoke_docker_available() { return 0; }
smoke_compose_service_rows() { printf '%s\n' 'sybil-api /sybil-api running healthy 0'; }
prover_calls=0
smoke_compose_service_curl() { prover_calls=$((prover_calls + 1)); return 1; }
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 0 && "$SKIPN" -eq 1 && "$SECONDS" -eq 0 && "$prover_calls" -eq 0 ]] \
    || { echo "FAIL: absent optional validity did not skip immediately" >&2; exit 1; }

# Docker/prover access is fail-closed only when the deploy recipe requires it;
# direct diagnostic runs retain an explicit skip.
reset_gate
smoke_docker_available() { return 1; }
REQUIRE_PROOF_FRESHNESS=1
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 1 && "$SKIPN" -eq 0 ]] \
    || { echo "FAIL: required proof check accepted missing Docker" >&2; exit 1; }
reset_gate
REQUIRE_PROOF_FRESHNESS=0
check_proof_freshness >/dev/null
[[ "$FAILN" -eq 0 && "$SKIPN" -eq 1 ]] \
    || { echo "FAIL: optional proof check did not report an explicit skip" >&2; exit 1; }

# The promotion gate must distinguish a healthy history process from a usable
# API-to-history query contract. This catches a stale same-image sidecar.
reset_gate
SERVICE_TOKEN=smoke-service
HISTORY_ACCOUNT=42
http() {
    HTTP_CODE=200
    HTTP_BODY='{"fills":[],"history_scope":"remote"}'
}
check_history_query_contract >/dev/null
[[ "$FAILN" -eq 0 && "$PASSN" -eq 1 ]] \
    || { echo "FAIL: compatible account history contract did not pass" >&2; exit 1; }

reset_gate
SERVICE_TOKEN=smoke-service
HISTORY_ACCOUNT=42
http() {
    HTTP_CODE=503
    HTTP_BODY='{"error":"Historical data is temporarily unavailable","code":"HISTORY_UNAVAILABLE"}'
}
check_history_query_contract >/dev/null
[[ "$FAILN" -eq 1 ]] \
    || { echo "FAIL: unavailable account history did not block promotion" >&2; exit 1; }

echo "post-deploy smoke tests: ok"
