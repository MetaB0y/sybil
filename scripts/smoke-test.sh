#!/usr/bin/env bash
set -euo pipefail

# E2E smoke test for sybil-api.
# Starts the server, exercises the main API flows, then tears down.

API="http://localhost:${SYBIL_PORT:-3000}"
PID=""

cleanup() {
    if [[ -n "$PID" ]]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

pass() { printf "  \033[32m✓\033[0m %s\n" "$1"; }
fail() { printf "  \033[31m✗\033[0m %s\n" "$1"; exit 1; }
step() { printf "\n\033[1m%s\033[0m\n" "$1"; }

# ── Build ───────────────────────────────────────────────────────────────────

step "Building sybil-api..."
cargo build --release -p sybil-api

# ── Start server ────────────────────────────────────────────────────────────

step "Starting sybil-api..."
./target/release/sybil-api --dev-mode --port "${SYBIL_PORT:-3000}" &
PID=$!

# Wait for server to be ready
for i in $(seq 1 30); do
    if curl -sf "$API/v1/health" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        fail "Server process died"
    fi
    sleep 0.5
done
curl -sf "$API/v1/health" >/dev/null || fail "Server did not start within 15s"
pass "Server is up"

# ── Health & metrics ────────────────────────────────────────────────────────

step "System endpoints"

HEALTH=$(curl -sf "$API/v1/health")
echo "$HEALTH" | grep -q '"status":"ok"' && pass "GET /v1/health" || fail "GET /v1/health"

METRICS=$(curl -sf "$API/metrics")
echo "$METRICS" | grep -q "sybil_" && pass "GET /metrics (prometheus)" || fail "GET /metrics"

STATE=$(curl -sf "$API/v1/state-root")
echo "$STATE" | grep -q "state_root" && pass "GET /v1/state-root" || fail "GET /v1/state-root"

# ── Account lifecycle ───────────────────────────────────────────────────────

step "Account lifecycle"

ACCT=$(curl -sf -X POST "$API/v1/accounts" \
    -H 'content-type: application/json' \
    -d '{"initial_balance_nanos": 100000000000}')
ACCT_ID=$(echo "$ACCT" | sed -n 's/.*"account_id":\([0-9]*\).*/\1/p')
[[ -n "$ACCT_ID" ]] && pass "POST /v1/accounts → id=$ACCT_ID (\$100)" || fail "POST /v1/accounts"

GET_ACCT=$(curl -sf "$API/v1/accounts/$ACCT_ID")
echo "$GET_ACCT" | grep -q "100000000000" && pass "GET /v1/accounts/$ACCT_ID (balance correct)" || fail "GET account"

# ── Market lifecycle ────────────────────────────────────────────────────────

step "Market lifecycle"

MKT=$(curl -sf -X POST "$API/v1/markets" \
    -H 'content-type: application/json' \
    -d '{"name": "Smoke test market", "description": "Will this test pass?", "category": "testing"}')
MKT_ID=$(echo "$MKT" | sed -n 's/.*"market_id":\([0-9]*\).*/\1/p')
[[ -n "$MKT_ID" ]] && pass "POST /v1/markets → id=$MKT_ID" || fail "POST /v1/markets"

GET_MKT=$(curl -sf "$API/v1/markets/$MKT_ID")
echo "$GET_MKT" | grep -q "Smoke test market" && pass "GET /v1/markets/$MKT_ID" || fail "GET market"

PRICES=$(curl -sf "$API/v1/markets/prices")
echo "$PRICES" | grep -q "prices" && pass "GET /v1/markets/prices" || fail "GET prices"

# ── Order submission ────────────────────────────────────────────────────────

step "Order submission"

ORDER=$(curl -sf -X POST "$API/v1/orders" \
    -H 'content-type: application/json' \
    -d "{\"account_id\": $ACCT_ID, \"orders\": [{\"type\": \"BuyYes\", \"market_id\": $MKT_ID, \"limit_price_nanos\": 600000000, \"quantity\": 10}]}")
echo "$ORDER" | grep -q "accepted" && pass "POST /v1/orders (BuyYes)" || fail "POST /v1/orders"

# Submit a counterparty so we get a fill
ACCT2=$(curl -sf -X POST "$API/v1/accounts" \
    -H 'content-type: application/json' \
    -d '{"initial_balance_nanos": 100000000000}')
ACCT2_ID=$(echo "$ACCT2" | sed -n 's/.*"account_id":\([0-9]*\).*/\1/p')

ORDER2=$(curl -sf -X POST "$API/v1/orders" \
    -H 'content-type: application/json' \
    -d "{\"account_id\": $ACCT2_ID, \"orders\": [{\"type\": \"BuyNo\", \"market_id\": $MKT_ID, \"limit_price_nanos\": 600000000, \"quantity\": 10}]}")
echo "$ORDER2" | grep -q "accepted" && pass "POST /v1/orders (BuyNo counterparty)" || fail "POST /v1/orders counterparty"

# ── Wait for block with fills ──────────────────────────────────────────────

step "Block production"

# Wait for a block that contains fills (up to 10s)
FILLS_FOUND=""
for i in $(seq 1 20); do
    BLOCK=$(curl -sf "$API/v1/blocks/latest" 2>/dev/null || echo "{}")
    if echo "$BLOCK" | grep -q '"fill_count":[1-9]'; then
        FILLS_FOUND="yes"
        break
    fi
    sleep 0.5
done

[[ "$FILLS_FOUND" == "yes" ]] && pass "Block produced with fills" || fail "No fills after 10s"

HEIGHT=$(echo "$BLOCK" | sed -n 's/.*"height":\([0-9]*\).*/\1/p')
pass "Latest block height: $HEIGHT"

# Check block by height
BLOCK_H=$(curl -sf "$API/v1/blocks/$HEIGHT")
echo "$BLOCK_H" | grep -q "fills" && pass "GET /v1/blocks/$HEIGHT" || fail "GET block by height"

# ── Portfolio & fills ───────────────────────────────────────────────────────

step "Portfolio & fills"

PORTFOLIO=$(curl -sf "$API/v1/accounts/$ACCT_ID/portfolio")
echo "$PORTFOLIO" | grep -q "positions" && pass "GET /v1/accounts/$ACCT_ID/portfolio" || fail "GET portfolio"

FILLS=$(curl -sf "$API/v1/accounts/$ACCT_ID/fills")
echo "$FILLS" | grep -q "fill_qty" && pass "GET /v1/accounts/$ACCT_ID/fills" || fail "GET fills"

# ── Market resolution ───────────────────────────────────────────────────────

step "Market resolution"

RESOLVE=$(curl -sf -X POST "$API/v1/markets/$MKT_ID/resolve" \
    -H 'content-type: application/json' \
    -d '{"payout_nanos": 1000000000}')
echo "$RESOLVE" | grep -q "resolved" && pass "POST /v1/markets/$MKT_ID/resolve (YES wins)" || fail "resolve market"

# ── Metrics check ───────────────────────────────────────────────────────────

step "Metrics verification"

METRICS2=$(curl -sf "$API/metrics")
echo "$METRICS2" | grep -q "sybil_block_height" && pass "sybil_block_height present" || fail "missing block_height metric"
echo "$METRICS2" | grep -q "sybil_http_requests_total" && pass "sybil_http_requests_total present" || fail "missing http_requests metric"
echo "$METRICS2" | grep -q "sybil_solve_time_seconds" && pass "sybil_solve_time_seconds present" || fail "missing solve_time metric"

# ── OpenAPI ─────────────────────────────────────────────────────────────────

step "OpenAPI spec"

SPEC=$(curl -sf "$API/openapi.json")
echo "$SPEC" | grep -q '"openapi"' && pass "GET /openapi.json" || fail "GET /openapi.json"

# ── Done ────────────────────────────────────────────────────────────────────

printf "\n\033[32;1mAll smoke tests passed.\033[0m\n"
