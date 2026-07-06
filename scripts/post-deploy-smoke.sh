#!/usr/bin/env bash
# Post-deploy smoke test against a LIVE Sybil stack (SYB-223).
#
# One command, run after a devnet/prod redeploy, that exercises the public and
# (dev-mode) private surfaces end-to-end and prints clear PASS/FAIL lines. Exits
# non-zero if any check FAILs. SKIP/WARN do not fail the run.
#
# Usage:
#   scripts/post-deploy-smoke.sh <base_url> [--service-token TOKEN]
#                                           [--block-interval SECONDS]
#
#   <base_url>          e.g. https://172-104-31-54.nip.io  or  http://localhost:3000
#   --service-token     bearer token for service-gated routes (prod, non-dev-mode)
#   --block-interval    expected block time in seconds (default 10); the liveness
#                       poll waits ~1.5 intervals between samples
#
# Checks:
#   1. /v1/health, /v1/state-root, /v1/blocks/latest advancing
#   2. /v1/markets has >=1 native and >=1 mirror market
#   3. account lifecycle: create -> register key -> fund -> signed order ->
#      cancel -> empty orders (dev-mode gated; SKIPs if unavailable)
#   4. signed bridge withdrawal -> GET withdrawal l1_status shape
#   5. /v1/blocks/ws?from_block=H-2 replay-then-live
#   6. /v1/bots/decisions returns 200
#
# Canonical signing bytes are NOT reimplemented here: steps 3/4 shell out to
# `cargo run -p sybil-client --example smoke_sign`, which uses the one canonical
# home (`sybil-signing`). If cargo/the crate are unavailable, signed steps SKIP.

set -uo pipefail

# ── Arguments ───────────────────────────────────────────────────────────────
BASE=""
SERVICE_TOKEN=""
INTERVAL=10

usage() {
    grep '^#' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --service-token) SERVICE_TOKEN="${2:-}"; shift 2 ;;
        --block-interval) INTERVAL="${2:-10}"; shift 2 ;;
        --*) echo "unknown flag: $1" >&2; usage 2 ;;
        *)
            if [[ -z "$BASE" ]]; then BASE="$1"; shift
            else echo "unexpected argument: $1" >&2; usage 2; fi
            ;;
    esac
done

[[ -z "$BASE" ]] && { echo "error: <base_url> is required" >&2; usage 2; }
BASE="${BASE%/}" # strip trailing slash

for tool in curl jq python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ── Reporting ───────────────────────────────────────────────────────────────
PASSN=0; FAILN=0; SKIPN=0
pass() { echo "[PASS] $*"; PASSN=$((PASSN + 1)); }
fail() { echo "[FAIL] $*"; FAILN=$((FAILN + 1)); }
skip() { echo "[SKIP] $*"; SKIPN=$((SKIPN + 1)); }
warn() { echo "[WARN] $*"; }
info() { echo "       $*"; }
section() { echo; echo "== $* =="; }

# ── HTTP helper: sets HTTP_CODE and HTTP_BODY ───────────────────────────────
http() {
    local method="$1" path="$2" body="${3:-}"
    local args=(-sS -m 30 -o "$TMP/body" -w '%{http_code}' -X "$method"
        "$BASE$path" -H 'Accept: application/json')
    [[ -n "$SERVICE_TOKEN" ]] && args+=(-H "Authorization: Bearer $SERVICE_TOKEN")
    if [[ -n "$body" ]]; then
        args+=(-H 'Content-Type: application/json' --data "$body")
    fi
    HTTP_CODE="$(curl "${args[@]}" 2>/dev/null || echo 000)"
    HTTP_BODY="$(cat "$TMP/body" 2>/dev/null || true)"
}

is_2xx() { [[ "$1" =~ ^2[0-9][0-9]$ ]]; }

# ── Signing helper (canonical bytes live in sybil-signing) ──────────────────
SIGN_BIN=""
setup_signing() {
    if ! command -v cargo >/dev/null 2>&1; then
        warn "cargo not found; signed-flow checks (3,4) will SKIP"
        return
    fi
    if [[ ! -f "$REPO_ROOT/crates/sybil-client/examples/smoke_sign.rs" ]]; then
        warn "smoke_sign example not found; signed-flow checks (3,4) will SKIP"
        return
    fi
    info "building smoke_sign signing helper (cargo)..."
    if cargo build -q --manifest-path "$REPO_ROOT/Cargo.toml" \
        -p sybil-client --example smoke_sign 2>"$TMP/build.log"; then
        SIGN_BIN="$REPO_ROOT/target/debug/examples/smoke_sign"
    else
        warn "smoke_sign build failed; signed-flow checks (3,4) will SKIP"
        sed 's/^/       /' "$TMP/build.log" | tail -20
    fi
}

# ── 1. Liveness ─────────────────────────────────────────────────────────────
HEAD_HEIGHT=0
check_liveness() {
    section "1. Liveness"

    http GET /v1/health
    if is_2xx "$HTTP_CODE"; then pass "/v1/health -> $HTTP_CODE"
    else fail "/v1/health -> $HTTP_CODE: $HTTP_BODY"; fi

    http GET /v1/state-root
    local root; root="$(echo "$HTTP_BODY" | jq -r '.state_root // empty' 2>/dev/null)"
    if is_2xx "$HTTP_CODE" && [[ -n "$root" ]]; then
        pass "/v1/state-root -> root ${root:0:16}..."
    else fail "/v1/state-root -> $HTTP_CODE: $HTTP_BODY"; fi

    http GET /v1/blocks/latest
    local h1; h1="$(echo "$HTTP_BODY" | jq -r '.height // empty' 2>/dev/null)"
    if ! is_2xx "$HTTP_CODE" || [[ -z "$h1" ]]; then
        fail "/v1/blocks/latest -> $HTTP_CODE: $HTTP_BODY"
        return
    fi
    if [[ "$h1" -gt 0 ]]; then pass "/v1/blocks/latest height=$h1 (>0)"
    else fail "/v1/blocks/latest height=$h1 is not >0"; fi

    local wait; wait="$(python3 -c "print(round($INTERVAL*1.5, 2))")"
    info "waiting ${wait}s (~1.5 block intervals) to confirm advancement..."
    sleep "$wait"

    http GET /v1/blocks/latest
    local h2; h2="$(echo "$HTTP_BODY" | jq -r '.height // empty' 2>/dev/null)"
    if is_2xx "$HTTP_CODE" && [[ -n "$h2" && "$h2" -gt "$h1" ]]; then
        pass "chain ADVANCING: $h1 -> $h2"
        HEAD_HEIGHT="$h2"
    else
        fail "chain not advancing: $h1 -> ${h2:-?} (is block production running?)"
        HEAD_HEIGHT="${h2:-$h1}"
    fi
}

# ── 2. Markets ──────────────────────────────────────────────────────────────
ORDER_MARKET=""
check_markets() {
    section "2. Markets"
    http GET /v1/markets
    if ! is_2xx "$HTTP_CODE"; then
        fail "/v1/markets -> $HTTP_CODE: $HTTP_BODY"
        return
    fi
    if ! echo "$HTTP_BODY" | jq -e 'type == "array"' >/dev/null 2>&1; then
        fail "/v1/markets did not return an array"
        return
    fi

    local native mirror
    native="$(echo "$HTTP_BODY" | jq '[.[] | select(.polymarket_condition_id == null and (.resolution_criteria // "") != "")] | length')"
    mirror="$(echo "$HTTP_BODY" | jq '[.[] | select(.polymarket_condition_id != null)] | length')"

    if [[ "$native" -ge 1 ]]; then pass "native markets: $native (>=1)"
    else fail "native markets: $native (need >=1 with null condition id + resolution_criteria)"; fi
    if [[ "$mirror" -ge 1 ]]; then pass "mirror markets: $mirror (>=1)"
    else fail "mirror markets: $mirror (need >=1 with polymarket_condition_id set)"; fi

    # Pick a market to trade against: prefer a native one, else the first.
    ORDER_MARKET="$(echo "$HTTP_BODY" | jq -r 'map(select(.polymarket_condition_id == null)) | (.[0].market_id // empty)')"
    [[ -z "$ORDER_MARKET" ]] && ORDER_MARKET="$(echo "$HTTP_BODY" | jq -r '(.[0].market_id // empty)')"
}

# ── 3+4. Account lifecycle + signed withdrawal (dev-mode gated) ─────────────
check_account_lifecycle() {
    section "3. Account lifecycle (signed order + cancel)"

    if [[ -z "$SIGN_BIN" ]]; then
        skip "signing helper unavailable; cannot exercise signed flow"
        skip "signed bridge withdrawal (depends on signing helper)"
        return
    fi
    if [[ -z "$ORDER_MARKET" ]]; then
        skip "no market available to trade against"
        skip "signed bridge withdrawal (depends on account lifecycle)"
        return
    fi

    # Create account (dev-mode gated). Non-2xx => dev endpoints unavailable.
    http POST /v1/accounts '{"initial_balance_nanos":1000000000000}'
    if ! is_2xx "$HTTP_CODE"; then
        warn "POST /v1/accounts -> $HTTP_CODE (dev endpoints appear disabled on this stack)"
        skip "account lifecycle requires dev-mode account endpoints"
        skip "signed bridge withdrawal requires dev-mode account endpoints"
        return
    fi
    local acct; acct="$(echo "$HTTP_BODY" | jq -r '.account_id')"
    pass "created account $acct"

    # Fresh ephemeral keypair (never persisted).
    local kp priv pub
    kp="$("$SIGN_BIN" keygen)"
    priv="$(echo "$kp" | jq -r '.private_key_hex')"
    pub="$(echo "$kp" | jq -r '.public_key_hex')"

    http POST "/v1/accounts/$acct/keys" "$(jq -n --arg pk "$pub" '{public_key_hex:$pk}')"
    if is_2xx "$HTTP_CODE"; then pass "registered P256 key"
    else fail "register key -> $HTTP_CODE: $HTTP_BODY"; return; fi

    http POST "/v1/accounts/$acct/fund" '{"amount_nanos":1000000000000}'
    if is_2xx "$HTTP_CODE"; then pass "funded account"
    else warn "fund -> $HTTP_CODE: $HTTP_BODY (continuing)"; fi

    # Signed order: rest a far-from-market buy so it does not immediately fill.
    local nonce1 osig ospk ossig obody
    nonce1="$(date +%s%3N)"
    osig="$("$SIGN_BIN" order --priv "$priv" --market "$ORDER_MARKET" \
        --nonce "$nonce1" --price 10000000 --qty 1000)"
    ospk="$(echo "$osig" | jq -r '.signer_pubkey_hex')"
    ossig="$(echo "$osig" | jq -r '.signature_hex')"
    obody="$(jq -n --arg pk "$ospk" --arg sig "$ossig" \
        --argjson m "$ORDER_MARKET" --argjson n "$nonce1" \
        '{signer_pubkey_hex:$pk, order:{market_ids:[$m], payoffs:[1,0], limit_price_nanos:10000000, max_fill:1000}, nonce:$n, signature_hex:$sig}')"

    http POST /v1/orders/signed "$obody"
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jq -r '.accepted // false')" == "true" ]]; then
        pass "signed order accepted (nonce $nonce1)"
    else
        fail "signed order -> $HTTP_CODE: $HTTP_BODY"
        return
    fi

    info "waiting ${INTERVAL}s (~1 block) for the order to rest..."
    sleep "$INTERVAL"

    http GET "/v1/accounts/$acct/orders"
    local count; count="$(echo "$HTTP_BODY" | jq 'length' 2>/dev/null || echo 0)"
    if [[ "$count" -gt 0 ]]; then
        local oid nonce2 csig cspk cssig cbody
        oid="$(echo "$HTTP_BODY" | jq -r '.[0].order_id')"
        nonce2="$((nonce1 + 1))"
        csig="$("$SIGN_BIN" cancel --priv "$priv" --account "$acct" \
            --order "$oid" --nonce "$nonce2")"
        cspk="$(echo "$csig" | jq -r '.signer_pubkey_hex')"
        cssig="$(echo "$csig" | jq -r '.signature_hex')"
        cbody="$(jq -n --arg pk "$cspk" --arg sig "$cssig" \
            --argjson a "$acct" --argjson o "$oid" --argjson n "$nonce2" \
            '{account_id:$a, order_id:$o, signer_pubkey_hex:$pk, nonce:$n, signature_hex:$sig}')"
        http POST /v1/orders/cancel/signed "$cbody"
        if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jq -r '.cancelled // false')" == "true" ]]; then
            pass "signed cancel of order $oid accepted"
        else
            fail "signed cancel -> $HTTP_CODE: $HTTP_BODY"
        fi
    else
        warn "order did not rest (filled or expired this batch); still verifying empty list"
    fi

    http GET "/v1/accounts/$acct/orders"
    count="$(echo "$HTTP_BODY" | jq 'length' 2>/dev/null || echo -1)"
    if is_2xx "$HTTP_CODE" && [[ "$count" == "0" ]]; then
        pass "orders list empty after cancel"
    else
        fail "orders list not empty (count=$count): $HTTP_BODY"
    fi

    check_signed_withdrawal "$acct" "$priv" "$nonce1"
}

# ── 4. Signed bridge withdrawal shape ───────────────────────────────────────
check_signed_withdrawal() {
    local acct="$1" priv="$2" base_nonce="$3"
    section "4. Signed bridge withdrawal"

    local expiry nonce3 wsig wspk wssig wbody wid
    expiry="$((HEAD_HEIGHT + 100000))"
    nonce3="$((base_nonce + 2))"
    local vault="1111111111111111111111111111111111111111"
    local recip="2222222222222222222222222222222222222222"
    local token="3333333333333333333333333333333333333333"

    wsig="$("$SIGN_BIN" withdrawal --priv "$priv" --account "$acct" \
        --chain-id 31337 --vault "$vault" --recipient "$recip" --token "$token" \
        --amount 1000 --expiry "$expiry" --nonce "$nonce3")"
    wspk="$(echo "$wsig" | jq -r '.signer_pubkey_hex')"
    wssig="$(echo "$wsig" | jq -r '.signature_hex')"
    wbody="$(jq -n --arg pk "$wspk" --arg sig "$wssig" \
        --argjson a "$acct" --argjson e "$expiry" --argjson n "$nonce3" \
        --arg v "$vault" --arg r "$recip" --arg t "$token" \
        '{withdrawal:{account_id:$a, chain_id:31337, vault_address_hex:$v, recipient_hex:$r, token_address_hex:$t, amount_token_units:1000, expiry_height:$e, nonce:$n}, signer_pubkey_hex:$pk, signature_hex:$sig}')"

    http POST /v1/bridge/withdrawals/signed "$wbody"
    if ! is_2xx "$HTTP_CODE"; then
        fail "POST /v1/bridge/withdrawals/signed -> $HTTP_CODE: $HTTP_BODY"
        return
    fi
    wid="$(echo "$HTTP_BODY" | jq -r '.withdrawal_id // empty')"
    if [[ -z "$wid" ]]; then
        fail "withdrawal response missing withdrawal_id: $HTTP_BODY"
        return
    fi
    pass "signed withdrawal created (id=$wid)"

    http GET "/v1/bridge/withdrawals/$wid"
    if is_2xx "$HTTP_CODE" && echo "$HTTP_BODY" | jq -e 'has("l1_status")' >/dev/null 2>&1; then
        pass "GET withdrawal $wid has l1_status=$(echo "$HTTP_BODY" | jq -r '.l1_status')"
    else
        fail "GET withdrawal $wid missing l1_status: $HTTP_CODE $HTTP_BODY"
    fi
}

# ── 5. WebSocket replay/resume ──────────────────────────────────────────────
check_ws_resume() {
    section "5. WebSocket resume (/v1/blocks/ws?from_block=H-2)"
    if [[ "$HEAD_HEIGHT" -le 0 ]]; then
        skip "no head height available for ws replay"
        return
    fi
    local ws_base from_block ws_url ws_timeout
    ws_base="$BASE"
    ws_base="${ws_base/#https:\/\//wss://}"
    ws_base="${ws_base/#http:\/\//ws://}"
    if [[ "$HEAD_HEIGHT" -gt 2 ]]; then from_block="$((HEAD_HEIGHT - 2))"; else from_block=1; fi
    ws_url="$ws_base/v1/blocks/ws?from_block=$from_block"
    ws_timeout="$(python3 -c "print(round($INTERVAL*2 + 5, 2))")"
    info "connecting $ws_url (head=$HEAD_HEIGHT, timeout=${ws_timeout}s)"

    if python3 "$SCRIPT_DIR/_ws_resume_check.py" "$ws_url" "$HEAD_HEIGHT" "$ws_timeout" \
        | grep -q '^ws_resume=pass'; then
        pass "ws replayed frames then a live frame"
    else
        fail "ws did not deliver replay-then-live (see diagnostics above)"
    fi
}

# ── 6. Bot decisions ────────────────────────────────────────────────────────
check_bots() {
    section "6. Bot decisions"
    http GET /v1/bots/decisions
    if is_2xx "$HTTP_CODE"; then pass "/v1/bots/decisions -> $HTTP_CODE"
    else fail "/v1/bots/decisions -> $HTTP_CODE: $HTTP_BODY"; fi
}

# ── Run ─────────────────────────────────────────────────────────────────────
echo "Sybil post-deploy smoke — target: $BASE (block interval ${INTERVAL}s)"
setup_signing
check_liveness
check_markets
check_account_lifecycle
check_ws_resume
check_bots

section "Summary"
echo "PASS=$PASSN  FAIL=$FAILN  SKIP=$SKIPN"
if [[ "$FAILN" -gt 0 ]]; then
    echo "RESULT: FAIL"
    exit 1
fi
echo "RESULT: OK"
exit 0
