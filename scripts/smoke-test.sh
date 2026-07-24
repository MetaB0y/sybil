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
    -d '{"provisioning_key":"smoke-test/account-1/v1","initial_balance_nanos":"100000000000"}')
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
    -d "{\"account_id\": $ACCT_ID, \"orders\": [{\"type\": \"BuyYes\", \"market_id\": $MKT_ID, \"limit_price_nanos\": \"600000000\", \"quantity\": 10}]}")
echo "$ORDER" | grep -q "accepted" && pass "POST /v1/orders (BuyYes)" || fail "POST /v1/orders"

# Submit a counterparty so we get a fill
ACCT2=$(curl -sf -X POST "$API/v1/accounts" \
    -H 'content-type: application/json' \
    -d '{"provisioning_key":"smoke-test/account-2/v1","initial_balance_nanos":"100000000000"}')
ACCT2_ID=$(echo "$ACCT2" | sed -n 's/.*"account_id":\([0-9]*\).*/\1/p')

ORDER2=$(curl -sf -X POST "$API/v1/orders" \
    -H 'content-type: application/json' \
    -d "{\"account_id\": $ACCT2_ID, \"orders\": [{\"type\": \"BuyNo\", \"market_id\": $MKT_ID, \"limit_price_nanos\": \"600000000\", \"quantity\": 10}]}")
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

# ── Persisted aggregate and account-history surfaces ────────────────────────
# Runs BEFORE market resolution so per-position fields like
# `avg_entry_price_nanos` still appear in the portfolio response (resolution
# zeroes positions and folds them into realized_pnl).

step "Aggregate read-model surfaces"

OVERVIEW=$(curl -sf "$API/v1/activity/overview")
echo "$OVERVIEW" | grep -q '"all_time"' && pass "GET /v1/activity/overview (all_time bucket)" || fail "activity overview missing all_time"
echo "$OVERVIEW" | grep -q '"last_24h"' && pass "GET /v1/activity/overview (last_24h bucket)" || fail "activity overview missing last_24h"
echo "$OVERVIEW" | grep -q '"unique_traders"' && pass "activity overview carries unique_traders (B1)" || fail "missing unique_traders"
echo "$OVERVIEW" | grep -q '"total_volume_nanos"' && pass "activity overview carries total_volume_nanos (B2)" || fail "missing total_volume_nanos"
echo "$OVERVIEW" | grep -q '"orders"' && pass "activity overview carries orders stats (B6)" || fail "missing orders stats"

OPENBATCH=$(curl -sf "$API/v1/markets/$MKT_ID/open-batch")
echo "$OPENBATCH" | grep -q '"unique_placers"' && pass "GET /v1/markets/{id}/open-batch (B1 unique_placers)" || fail "open-batch missing unique_placers"
echo "$OPENBATCH" | grep -q '"indicative_volume_nanos"' && pass "open-batch carries indicative_volume_nanos (C2)" || fail "missing indicative_volume_nanos"
echo "$OPENBATCH" | grep -q '"indicative_computed_at_ms"' && pass "open-batch carries indicative_computed_at_ms (C2)" || fail "missing indicative_computed_at_ms"

MKT_FULL=$(curl -sf "$API/v1/markets/$MKT_ID")
echo "$MKT_FULL" | grep -q '"trader_count"' && pass "MarketResponse carries trader_count (B1)" || fail "missing trader_count"
echo "$MKT_FULL" | grep -q '"volume_24h_nanos"' && pass "MarketResponse carries volume_24h_nanos (B2)" || fail "missing volume_24h_nanos"
echo "$MKT_FULL" | grep -q '"liquidity_avg10_nanos"' && pass "MarketResponse carries liquidity_avg10_nanos (B4)" || fail "missing liquidity_avg10_nanos"
echo "$MKT_FULL" | grep -q '"liquidity_band_nanos"' && pass "MarketResponse carries liquidity_band_nanos (B4)" || fail "missing liquidity_band_nanos"
echo "$MKT_FULL" | grep -q '"orders_placed_total"' && pass "MarketResponse carries orders_placed_total (B6)" || fail "missing orders_placed_total"
echo "$MKT_FULL" | grep -q '"orders_matched_total"' && pass "MarketResponse carries orders_matched_total (B6)" || fail "missing orders_matched_total"
echo "$MKT_FULL" | grep -q '"orders_unmatched_total"' && pass "MarketResponse carries orders_unmatched_total (B6)" || fail "missing orders_unmatched_total"

PORTFOLIO_FULL=$(curl -sf "$API/v1/accounts/$ACCT_ID/portfolio")
echo "$PORTFOLIO_FULL" | grep -q '"first_deposit_ms"' && pass "PortfolioResponse carries first_deposit_ms (B8)" || fail "missing first_deposit_ms"
echo "$PORTFOLIO_FULL" | grep -q '"total_fill_count"' && pass "PortfolioResponse carries total_fill_count (B8)" || fail "missing total_fill_count"
echo "$PORTFOLIO_FULL" | grep -q '"realized_pnl_nanos"' && pass "PortfolioResponse carries realized_pnl_nanos (C1)" || fail "missing realized_pnl_nanos"
echo "$PORTFOLIO_FULL" | grep -q '"unrealized_pnl_nanos"' && pass "PortfolioResponse carries unrealized_pnl_nanos (C1)" || fail "missing unrealized_pnl_nanos"
echo "$PORTFOLIO_FULL" | grep -q '"avg_entry_price_nanos"' && pass "PositionValueResponse carries avg_entry_price_nanos (C1)" || fail "missing avg_entry_price_nanos"

# ── Market resolution ───────────────────────────────────────────────────────

step "Market resolution"

RESOLVE=$(curl -sf -X POST "$API/v1/markets/$MKT_ID/resolve" \
    -H 'content-type: application/json' \
    -d '{"payout_nanos": 1000000000}')
echo "$RESOLVE" | grep -q "resolved" && pass "POST /v1/markets/$MKT_ID/resolve (YES wins)" || fail "resolve market"

# ── Realized PnL after resolution (C1's apply_resolution hook) ──────────────
# Wait for one block so resolution settles, then re-fetch portfolio. With
# YES winning at $1 payout and ACCT_ID long 10 YES at $0.60, realized PnL
# should be a positive 4×nanos-per-share = $4 (= 4_000_000_000 nanos).
sleep 1
PORTFOLIO_POST=$(curl -sf "$API/v1/accounts/$ACCT_ID/portfolio")
REALIZED_POST=$(echo "$PORTFOLIO_POST" | sed -n 's/.*"realized_pnl_nanos":\(-\{0,1\}[0-9]*\).*/\1/p')
if [[ -n "$REALIZED_POST" && "$REALIZED_POST" != "0" ]]; then
    pass "realized_pnl_nanos populated after resolution (C1 apply_resolution): $REALIZED_POST"
else
    fail "realized_pnl_nanos still zero after resolution (got: '$REALIZED_POST')"
fi

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

# Schema-level checks for the new wire surfaces. These bind against the
# generated openapi spec, so they fail loudly if a Phase B/C/D wire field
# regresses or gets renamed.
echo "$SPEC" | grep -q 'order_cancelled' && pass "openapi exposes order_cancelled SystemEvent variant (D1)" || fail "openapi missing order_cancelled"
echo "$SPEC" | grep -q '"indicative_yes_price_nanos"' && pass "openapi exposes indicative_yes_price_nanos (C2)" || fail "openapi missing indicative_yes_price_nanos"
echo "$SPEC" | grep -q '"by_market"' && pass "openapi exposes BlockResponse.by_market (A1)" || fail "openapi missing by_market"

# ── Done ────────────────────────────────────────────────────────────────────

printf "\n\033[32;1mAll smoke tests passed.\033[0m\n"
