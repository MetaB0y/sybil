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

echo "smoke-common tests: ok"
