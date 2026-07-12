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

reset_gate() {
    PASSN=0; FAILN=0; SKIPN=0; RESULTS=(); ORDER_MARKET=""
    SECONDS=0
}

sleep() {
    SECONDS=$((SECONDS + $1))
}

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

echo "post-deploy smoke tests: ok"
