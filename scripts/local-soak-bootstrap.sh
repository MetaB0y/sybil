#!/bin/sh
set -eu

api_base=${SYBIL_SOAK_API_BASE:-http://sybil-api:3000}

case "$api_base" in
    http://sybil-api:3000) ;;
    *)
        echo "refusing non-local soak API target: $api_base" >&2
        exit 2
        ;;
esac

account_status() {
    curl -sS -o /dev/null -w '%{http_code}' "$api_base/v1/accounts/$1"
}

existing=0
for account_id in $(seq 1 16); do
    status=$(account_status "$account_id")
    case "$status" in
        200) existing=$((existing + 1)) ;;
        404) ;;
        *)
            echo "unexpected account lookup status for $account_id: $status" >&2
            exit 1
            ;;
    esac
done

if [ "$existing" -eq 16 ]; then
    echo "local soak actor accounts already exist; preserving balances and positions"
    exit 0
fi
if [ "$existing" -ne 0 ]; then
    echo "refusing partial actor bootstrap ($existing/16 fixed accounts exist); run 'just local-soak-clean'" >&2
    exit 1
fi

create_account() {
    expected_id=$1
    balance_nanos=$2
    response=$(curl -fsS \
        -H 'Content-Type: application/json' \
        -d "{\"initial_balance_nanos\":$balance_nanos}" \
        "$api_base/v1/accounts")
    actual_id=$(printf '%s' "$response" | sed -n 's/.*"account_id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
    if [ "$actual_id" != "$expected_id" ]; then
        echo "actor account allocation mismatch: expected $expected_id, got ${actual_id:-unparseable}" >&2
        exit 1
    fi
}

# The sequencer allocates ordinary accounts from zero. Keep account 0 as an
# unfunded local sentinel so the sixteen role-bound actors have stable ids 1–16.
# A previous interrupted bootstrap may already have created it.
sentinel_status=$(account_status 0)
case "$sentinel_status" in
    200) ;;
    404) create_account 0 0 ;;
    *)
        echo "unexpected sentinel account lookup status: $sentinel_status" >&2
        exit 1
        ;;
esac

# $5,000,000 MM collateral plus the unchanged $300,000 aggregate noise
# capital, divided evenly across fifteen durable principals ($20,000 each).
create_account 1 5000000000000000
for account_id in $(seq 2 16); do
    create_account "$account_id" 20000000000000
done

echo "created fixed local actor accounts: MM=1, noise=2..16"
