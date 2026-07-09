#!/usr/bin/env bash
# Post-deploy smoke GATE against a LIVE Sybil stack (SYB-223, hardened by SYB-240).
#
# This is the LAST deploy step: it runs against the live stack and BLOCKS
# promotion on any broken core flow. It is fail-closed — it exits non-zero if
# ANY core check FAILs. Unlike the original SYB-223 script, the core browser and
# trading flows are HARD assertions, never silent SKIPs:
#
#   * CORS preflight from the real app origin (the browser-breakage class).
#   * Passkey onboarding: unauthenticated account create + first-key bootstrap
#     (the HTTP-401 regression that shipped would FAIL here, not skip).
#   * Fills-after-seed: a deterministic crossing seed MUST increase matched
#     orders (the zero-fills regression would FAIL here, not skip).
#   * Service-token gating matrix: gated routes 401 without the token and
#     2xx/auth-pass with it; public routes stay public.
#
# Only two tools are required: curl and python3 (NO jq — the deploy box does not
# have it). Docker container-health and the signed-order flow are extra, harder
# assertions when their prerequisites are present.
#
# Usage:
#   scripts/post-deploy-smoke.sh [base_url] [--service-token TOKEN]
#                                           [--app-origin ORIGIN]
#                                           [--block-interval SECONDS]
#                                           [--require-signer]
#
# Configuration (flags override env; env overrides defaults):
#   base_url / SYBIL_SMOKE_BASE          API root host
#                                        (default https://172-104-31-54.nip.io;
#                                        the API is at the ROOT host, not api.*)
#   --service-token / SYBIL_SERVICE_TOKEN   bearer for service-gated routes
#   --app-origin / SYBIL_SMOKE_APP_ORIGIN   browser origin for the CORS check
#                                        (default https://app.172-104-31-54.nip.io)
#   --block-interval / SYBIL_SMOKE_INTERVAL block time seconds (default 10)
#   --require-signer / SYBIL_SMOKE_REQUIRE_SIGNER=1
#                                        FAIL (not SKIP) if the signed-order
#                                        signer is unavailable. Set this in the
#                                        deploy image where the signer ships.
#
#   SYBIL_SMOKE_DOCKER_SSH   run the container-health probe over this ssh target
#                            (e.g. root@172.104.31.54) instead of local docker.
#   SYBIL_COMPOSE_PROJECT    compose project label to enumerate (default sybil).
#   SYBIL_SMOKE_SIGN_BIN     path to a prebuilt smoke_sign binary (skips cargo).
#
# Exit: 0 only if FAIL=0. Any FAIL exits 1 and blocks promotion.

set -uo pipefail

# ── Configuration ───────────────────────────────────────────────────────────
BASE="${SYBIL_SMOKE_BASE:-https://172-104-31-54.nip.io}"
APP_ORIGIN="${SYBIL_SMOKE_APP_ORIGIN:-https://app.172-104-31-54.nip.io}"
SERVICE_TOKEN="${SYBIL_SERVICE_TOKEN:-}"
INTERVAL="${SYBIL_SMOKE_INTERVAL:-10}"
REQUIRE_SIGNER="${SYBIL_SMOKE_REQUIRE_SIGNER:-0}"
DOCKER_SSH="${SYBIL_SMOKE_DOCKER_SSH:-}"
COMPOSE_PROJECT="${SYBIL_COMPOSE_PROJECT:-sybil}"
BASE_SET_BY_ARG=0

usage() {
    grep '^#' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --service-token) SERVICE_TOKEN="${2:-}"; shift 2 ;;
        --app-origin) APP_ORIGIN="${2:-}"; shift 2 ;;
        --block-interval) INTERVAL="${2:-10}"; shift 2 ;;
        --require-signer) REQUIRE_SIGNER=1; shift ;;
        --*) echo "unknown flag: $1" >&2; usage 2 ;;
        *)
            if [[ "$BASE_SET_BY_ARG" -eq 0 ]]; then BASE="$1"; BASE_SET_BY_ARG=1; shift
            else echo "unexpected argument: $1" >&2; usage 2; fi
            ;;
    esac
done

BASE="${BASE%/}"           # strip trailing slash
APP_ORIGIN="${APP_ORIGIN%/}"

for tool in curl python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ── Reporting ───────────────────────────────────────────────────────────────
PASSN=0; FAILN=0; SKIPN=0
declare -a RESULTS=()
pass() { echo "[PASS] $*"; PASSN=$((PASSN + 1)); RESULTS+=("PASS|$*"); }
fail() { echo "[FAIL] $*"; FAILN=$((FAILN + 1)); RESULTS+=("FAIL|$*"); }
skip() { echo "[SKIP] $*"; SKIPN=$((SKIPN + 1)); RESULTS+=("SKIP|$*"); }
warn() { echo "[WARN] $*"; }
info() { echo "       $*"; }
section() { echo; echo "== $* =="; }

# ── JSON extraction (python3; no jq on the deploy box) ───────────────────────
# usage: echo "$json" | jget "dotted.path"   ->  prints scalar, "" if absent.
# Path segments are dict keys or list indices, e.g. all_time.orders.matched, 0.market_id
# NOTE: the program is passed via -c (NOT a heredoc) so that the JSON piped on
# stdin actually reaches json.load — `python3 - <<EOF` would consume stdin for
# the program source instead.
jget() {
    python3 -c '
import sys, json
path = sys.argv[1]
try:
    cur = json.load(sys.stdin)
except Exception:
    sys.exit(0)
for seg in (path.split(".") if path else []):
    if seg == "":
        continue
    try:
        if isinstance(cur, list):
            cur = cur[int(seg)]
        elif isinstance(cur, dict):
            cur = cur.get(seg)
        else:
            cur = None
    except Exception:
        cur = None
    if cur is None:
        break
if isinstance(cur, bool):
    print("true" if cur else "false")
elif cur is not None:
    print(cur)
' "$1"
}

# ── HTTP helper: sets HTTP_CODE and HTTP_BODY ───────────────────────────────
# usage: http METHOD PATH [BODY] [AUTH]   AUTH in none(default)|token|bad
http() {
    local method="$1" path="$2" body="${3:-}" auth="${4:-none}"
    local args=(-sS -m 30 -o "$TMP/body" -w '%{http_code}' -X "$method"
        "$BASE$path" -H 'Accept: application/json')
    case "$auth" in
        token) [[ -n "$SERVICE_TOKEN" ]] && args+=(-H "Authorization: Bearer $SERVICE_TOKEN") ;;
        bad)   args+=(-H "Authorization: Bearer smoke-invalid-token") ;;
        none)  : ;;
    esac
    if [[ -n "$body" ]]; then
        args+=(-H 'Content-Type: application/json' --data "$body")
    fi
    HTTP_CODE="$(curl "${args[@]}" 2>/dev/null || echo 000)"
    HTTP_BODY="$(cat "$TMP/body" 2>/dev/null || true)"
}

is_2xx() { [[ "$1" =~ ^2[0-9][0-9]$ ]]; }

# ── 1. Service health ───────────────────────────────────────────────────────
HEAD_HEIGHT=0
GENESIS_HASH=""
check_liveness() {
    section "1a. API liveness"

    http GET /v1/health
    GENESIS_HASH="$(echo "$HTTP_BODY" | jget genesis_hash)"
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget status)" == "ok" ]]; then
        pass "/v1/health -> ok (height=$(echo "$HTTP_BODY" | jget height))"
    else
        fail "/v1/health -> $HTTP_CODE: $HTTP_BODY"
    fi

    http GET /v1/state-root
    local root; root="$(echo "$HTTP_BODY" | jget state_root)"
    if is_2xx "$HTTP_CODE" && [[ -n "$root" ]]; then
        pass "/v1/state-root -> ${root:0:16}..."
    else fail "/v1/state-root -> $HTTP_CODE: $HTTP_BODY"; fi

    http GET /v1/blocks/latest
    local h1; h1="$(echo "$HTTP_BODY" | jget height)"
    if ! is_2xx "$HTTP_CODE" || [[ -z "$h1" ]]; then
        fail "/v1/blocks/latest -> $HTTP_CODE: $HTTP_BODY"; return
    fi
    if [[ "$h1" -gt 0 ]]; then pass "/v1/blocks/latest height=$h1 (>0)"
    else fail "/v1/blocks/latest height=$h1 is not >0"; fi

    local wait; wait="$(python3 -c "print(round($INTERVAL*1.5, 2))")"
    info "waiting ${wait}s (~1.5 block intervals) to confirm advancement..."
    sleep "$wait"

    http GET /v1/blocks/latest
    local h2; h2="$(echo "$HTTP_BODY" | jget height)"
    if is_2xx "$HTTP_CODE" && [[ -n "$h2" && "$h2" -gt "$h1" ]]; then
        pass "chain ADVANCING: $h1 -> $h2"
        HEAD_HEIGHT="$h2"
    else
        fail "chain not advancing: $h1 -> ${h2:-?} (is block production running?)"
        HEAD_HEIGHT="${h2:-$h1}"
    fi
}

# Container health for every compose service. Local docker, or over ssh.
docker_run() {
    if [[ -n "$DOCKER_SSH" ]]; then
        ssh -o BatchMode=yes -o ConnectTimeout=10 "$DOCKER_SSH" "$*" 2>/dev/null
    else
        eval "$*" 2>/dev/null
    fi
}
check_services() {
    section "1b. Container health (compose project '$COMPOSE_PROJECT')"
    local docker_ok=1
    if [[ -n "$DOCKER_SSH" ]]; then
        docker_run "command -v docker" >/dev/null || docker_ok=0
    else
        command -v docker >/dev/null 2>&1 || docker_ok=0
    fi
    if [[ "$docker_ok" -ne 1 ]]; then
        skip "docker unavailable ($([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local)); container-health matrix needs an on-box run (SYBIL_SMOKE_DOCKER_SSH)"
        return
    fi

    # One line per container: "<name> <status> <health|none>". Kept as a single
    # pipeline so IDs never travel through the (ssh) command string.
    local rows; rows="$(docker_run "docker ps -aq --filter label=com.docker.compose.project=$COMPOSE_PROJECT | xargs -r docker inspect --format '{{.Name}} {{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}'")"
    if [[ -z "$rows" ]]; then
        fail "no containers found for compose project '$COMPOSE_PROJECT'"
        return
    fi
    local saw_api=0
    while read -r name status health; do
        [[ -z "$name" ]] && continue
        name="${name#/}"
        [[ "$name" == *sybil-api* ]] && saw_api=1
        if [[ "$status" == "running" && ( "$health" == "none" || "$health" == "healthy" ) ]]; then
            pass "service $name: $status/$health"
        else
            fail "service $name: $status/$health (not running-and-healthy)"
        fi
    done <<< "$rows"
    if [[ "$saw_api" -ne 1 ]]; then
        fail "required service sybil-api not found in project '$COMPOSE_PROJECT'"
    fi
}

# ── 2. CORS preflight from the app origin ───────────────────────────────────
check_cors() {
    section "2. CORS preflight (browser origin: $APP_ORIGIN)"
    local path="/v1/accounts"
    local hdr code allow
    curl -sS -m 20 -D "$TMP/cors_hdr" -o /dev/null -X OPTIONS "$BASE$path" \
        -H "Origin: $APP_ORIGIN" \
        -H 'Access-Control-Request-Method: POST' \
        -H 'Access-Control-Request-Headers: content-type' >/dev/null 2>&1
    code="$(awk 'toupper($1) ~ /^HTTP/ {c=$2} END{print c}' "$TMP/cors_hdr")"
    allow="$(awk 'BEGIN{IGNORECASE=1} /^access-control-allow-origin:/ {sub(/^[^:]*:[ \t]*/,""); gsub(/\r/,""); print; exit}' "$TMP/cors_hdr")"
    local methods; methods="$(awk 'BEGIN{IGNORECASE=1} /^access-control-allow-methods:/ {sub(/^[^:]*:[ \t]*/,""); gsub(/\r/,""); print; exit}' "$TMP/cors_hdr")"

    if [[ -n "$code" ]] && is_2xx "$code"; then
        pass "OPTIONS $path from app origin -> $code"
    else
        fail "OPTIONS $path from app origin -> ${code:-no-response} (preflight rejected)"
    fi
    if [[ "$allow" == "$APP_ORIGIN" ]]; then
        pass "access-control-allow-origin == $APP_ORIGIN"
    else
        fail "access-control-allow-origin='$allow' (expected '$APP_ORIGIN') — browser POST /v1/accounts would be blocked"
    fi
    if echo "$methods" | grep -qi 'POST'; then
        pass "access-control-allow-methods includes POST ($methods)"
    else
        fail "access-control-allow-methods='$methods' does not allow POST"
    fi
}

# ── 3. Passkey onboarding (unauthenticated create + first-key bootstrap) ─────
# These MUST be hard assertions: the shipped regression made these 401.
check_onboarding() {
    section "3. Passkey onboarding (no service token)"

    # 3a. create account WITHOUT any token -> 200
    http POST /v1/accounts '{"initial_balance_nanos":1000000000000}' none
    local acct; acct="$(echo "$HTTP_BODY" | jget account_id)"
    if is_2xx "$HTTP_CODE" && [[ -n "$acct" ]]; then
        pass "POST /v1/accounts (no token) -> $HTTP_CODE, account_id=$acct"
    else
        fail "POST /v1/accounts (no token) -> $HTTP_CODE: $HTTP_BODY (passkey onboarding gated?)"
        return
    fi

    # 3b. over-cap initial_balance_nanos (> 5_000_000_000_000) -> 400
    http POST /v1/accounts '{"initial_balance_nanos":5000000000001}' none
    if [[ "$HTTP_CODE" == "400" ]]; then
        pass "over-cap initial_balance_nanos -> 400 (demo cap enforced)"
    else
        fail "over-cap initial_balance_nanos -> $HTTP_CODE (expected 400): $HTTP_BODY"
    fi

    # 3c. first-key bootstrap: register a fresh P256 key on a fresh account -> 200
    local pub; pub="$(python3 - <<'PY'
try:
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.hazmat.primitives import serialization
    k = ec.generate_private_key(ec.SECP256R1())
    print(k.public_key().public_bytes(
        serialization.Encoding.X962,
        serialization.PublicFormat.UncompressedPoint).hex())
except Exception:
    pass
PY
)"
    if [[ -z "$pub" ]]; then
        skip "python 'cryptography' not available; cannot mint a P256 key for the key-bootstrap check"
        return
    fi
    http POST "/v1/accounts/$acct/keys" "{\"public_key_hex\":\"$pub\"}" none
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget success)" == "true" ]]; then
        pass "first-key bootstrap POST /v1/accounts/$acct/keys -> $HTTP_CODE"
    else
        fail "first-key bootstrap -> $HTTP_CODE: $HTTP_BODY"
    fi

    # 3d. second key must be rejected: first-key-only bootstrap -> 409
    local pub2; pub2="$(python3 - <<'PY'
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives import serialization
k = ec.generate_private_key(ec.SECP256R1())
print(k.public_key().public_bytes(
    serialization.Encoding.X962,
    serialization.PublicFormat.UncompressedPoint).hex())
PY
)"
    http POST "/v1/accounts/$acct/keys" "{\"public_key_hex\":\"$pub2\"}" none
    if [[ "$HTTP_CODE" == "409" ]]; then
        pass "second key bootstrap -> 409 (first-key-only enforced)"
    else
        fail "second key bootstrap -> $HTTP_CODE (expected 409): $HTTP_BODY"
    fi
}

# ── 4. Markets present (needed to trade) ─────────────────────────────────────
ORDER_MARKET=""
check_markets() {
    section "4. Markets"
    http GET /v1/markets
    if ! is_2xx "$HTTP_CODE"; then
        fail "/v1/markets -> $HTTP_CODE: $HTTP_BODY"; return
    fi
    local counts; counts="$(echo "$HTTP_BODY" | python3 -c '
import sys, json
try:
    a = json.load(sys.stdin)
    assert isinstance(a, list)
except Exception:
    print("ERR 0 0"); sys.exit(0)
native = [m for m in a if m.get("polymarket_condition_id") is None and (m.get("resolution_criteria") or "") != ""]
mirror = [m for m in a if m.get("polymarket_condition_id") is not None]
cand = [m for m in a if m.get("polymarket_condition_id") is None] or a
pick = cand[0].get("market_id") if cand else None
print("OK", len(native), len(mirror), pick if pick is not None else "")
')"
    read -r ok native mirror pick <<< "$counts"
    if [[ "$ok" != "OK" ]]; then fail "/v1/markets did not return an array"; return; fi
    ORDER_MARKET="$pick"
    if [[ "$native" -ge 1 ]]; then pass "native markets: $native (>=1)"
    else fail "native markets: $native (need >=1)"; fi
    if [[ "$mirror" -ge 1 ]]; then pass "mirror markets: $mirror (>=1)"
    else warn "mirror markets: $mirror (no Polymarket mirror present)"; fi
    [[ -n "$ORDER_MARKET" ]] && info "trading against market_id=$ORDER_MARKET"
}

# ── 5. Order placement + deterministic fills gate ────────────────────────────
# Create two demo accounts, submit a crossing BuyYes/BuyNo pair on the same
# market (unsigned /v1/orders path, now service-token gated — seed as trusted infra),
# and assert matched orders INCREASE.
check_orders_and_fills() {
    section "5. Order placement + fills-after-seed gate"
    if [[ -z "$ORDER_MARKET" ]]; then
        fail "no market available; cannot place orders or seed fills"
        return
    fi

    http GET /v1/activity/overview
    local before; before="$(echo "$HTTP_BODY" | jget all_time.orders.matched)"
    [[ -z "$before" ]] && before=0
    info "baseline all_time.orders.matched = $before"

    # Two funded demo accounts (initial_balance == demo cap; no service token needed).
    local a c
    http POST /v1/accounts '{"initial_balance_nanos":5000000000000}' none
    a="$(echo "$HTTP_BODY" | jget account_id)"
    http POST /v1/accounts '{"initial_balance_nanos":5000000000000}' none
    c="$(echo "$HTTP_BODY" | jget account_id)"
    if [[ -z "$a" || -z "$c" ]]; then
        fail "could not create two demo accounts for the fills seed"
        return
    fi
    info "seed accounts: $a (BuyYes), $c (BuyNo)"

    # Crossing pair at 0.99 each: complementary demand that must clear.
    http POST /v1/orders \
        "{\"account_id\":$a,\"orders\":[{\"type\":\"BuyYes\",\"market_id\":$ORDER_MARKET,\"limit_price_nanos\":990000000,\"quantity\":1000}],\"time_in_force\":\"GTC\"}" token
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget accepted)" == "true" ]]; then
        pass "order placement: BuyYes accepted (acct $a)"
    else
        fail "order placement: BuyYes -> $HTTP_CODE: $HTTP_BODY"
    fi
    http POST /v1/orders \
        "{\"account_id\":$c,\"orders\":[{\"type\":\"BuyNo\",\"market_id\":$ORDER_MARKET,\"limit_price_nanos\":990000000,\"quantity\":1000}],\"time_in_force\":\"GTC\"}" token
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget accepted)" == "true" ]]; then
        pass "order placement: BuyNo accepted (acct $c)"
    else
        fail "order placement: BuyNo -> $HTTP_CODE: $HTTP_BODY"
    fi

    # Poll for matched to increase over ~ a few blocks.
    local deadline after now
    deadline="$(python3 -c "import time;print(round(time.time()+$INTERVAL*4+5,2))")"
    after="$before"
    info "polling all_time.orders.matched to exceed $before (up to $(python3 -c "print(round($INTERVAL*4+5))")s)..."
    while :; do
        sleep 3
        http GET /v1/activity/overview
        after="$(echo "$HTTP_BODY" | jget all_time.orders.matched)"
        [[ -z "$after" ]] && after=0
        now="$(python3 -c "import time;print(round(time.time(),2))")"
        [[ "$after" -gt "$before" ]] && break
        python3 -c "import sys;sys.exit(0 if $now < $deadline else 1)" || break
    done
    if [[ "$after" -gt "$before" ]]; then
        pass "FILLS gate: matched increased $before -> $after after deterministic seed"
    else
        fail "FILLS gate: matched did NOT increase ($before -> $after) — matching engine not filling crossing orders"
    fi
}

# ── 6. Service-token gating matrix ───────────────────────────────────────────
check_gating() {
    section "6. Service-token gating matrix"

    # Discover a real account id to fund (fund requires an existing account).
    http POST /v1/accounts '{"initial_balance_nanos":1000000000}' none
    local acct; acct="$(echo "$HTTP_BODY" | jget account_id)"
    [[ -z "$acct" ]] && acct=1

    # A well-formed (64 hex) but almost-certainly-absent leaf key so the
    # with-token call exercises auth, not payload validation.
    local leaf="0000000000000000000000000000000000000000000000000000000000000000"
    local -a gated=(
        "GET|/v1/da/1/payload"
        "GET|/v1/proofs/state/$leaf"
        "POST|/v1/accounts/$acct/fund"
    )
    local entry method path body
    for entry in "${gated[@]}"; do
        method="${entry%%|*}"; path="${entry#*|}"
        body=""; [[ "$method" == "POST" ]] && body='{"amount_nanos":1000}'

        # WITHOUT token -> 401
        http "$method" "$path" "$body" none
        if [[ "$HTTP_CODE" == "401" ]]; then
            pass "gated $method $path (no token) -> 401"
        else
            fail "gated $method $path (no token) -> $HTTP_CODE (expected 401): $HTTP_BODY"
        fi

        # WITH token -> auth must pass (never 401/403); ideally 2xx.
        if [[ -z "$SERVICE_TOKEN" ]]; then
            skip "$method $path with token: no SYBIL_SERVICE_TOKEN provided"
        else
            http "$method" "$path" "$body" token
            if is_2xx "$HTTP_CODE"; then
                pass "gated $method $path (token) -> $HTTP_CODE"
            elif [[ "$HTTP_CODE" == "401" || "$HTTP_CODE" == "403" ]]; then
                fail "gated $method $path (token) -> $HTTP_CODE (valid token rejected)"
            else
                pass "gated $method $path (token) -> $HTTP_CODE (auth passed; non-2xx is payload/resource, not gating)"
            fi
        fi
    done

    # WRONG token -> 403 (bonus: constant-time compare must reject).
    if [[ -n "$SERVICE_TOKEN" ]]; then
        http POST "/v1/accounts/$acct/fund" '{"amount_nanos":1000}' bad
        if [[ "$HTTP_CODE" == "403" ]]; then
            pass "wrong token -> 403"
        else
            fail "wrong token -> $HTTP_CODE (expected 403)"
        fi
    fi

    # Public endpoints must STAY public (no token needed).
    local pub
    for pub in /v1/health /v1/markets /v1/activity/overview; do
        http GET "$pub" "" none
        if is_2xx "$HTTP_CODE"; then pass "public $pub (no token) -> $HTTP_CODE"
        else fail "public $pub (no token) -> $HTTP_CODE (regressed to gated?)"; fi
    done
}

# ── 7. Signed order acceptance (extra; hard when signer required) ────────────
SIGN_BIN="${SYBIL_SMOKE_SIGN_BIN:-}"
setup_signing() {
    if [[ -n "$SIGN_BIN" && -x "$SIGN_BIN" ]]; then return; fi
    local prebuilt="$REPO_ROOT/target/debug/examples/smoke_sign"
    if [[ -x "$prebuilt" ]]; then SIGN_BIN="$prebuilt"; return; fi
    if ! command -v cargo >/dev/null 2>&1 \
       || [[ ! -f "$REPO_ROOT/crates/sybil-client/examples/smoke_sign.rs" ]]; then
        SIGN_BIN=""; return
    fi
    info "building smoke_sign signing helper (cargo)..."
    if cargo build -q --manifest-path "$REPO_ROOT/Cargo.toml" \
        -p sybil-client --example smoke_sign 2>"$TMP/build.log"; then
        SIGN_BIN="$REPO_ROOT/target/debug/examples/smoke_sign"
    else
        SIGN_BIN=""
        sed 's/^/       /' "$TMP/build.log" | tail -10
    fi
}
check_signed_order() {
    section "7. Signed order acceptance"
    setup_signing
    if [[ -z "$SIGN_BIN" ]]; then
        if [[ "$REQUIRE_SIGNER" == "1" ]]; then
            fail "signer unavailable but --require-signer set (ship smoke_sign in the deploy image)"
        else
            skip "signer (smoke_sign) unavailable; set SYBIL_SMOKE_REQUIRE_SIGNER=1 in the deploy gate to make this a hard check"
        fi
        return
    fi
    if [[ -z "$ORDER_MARKET" || -z "$GENESIS_HASH" ]]; then
        fail "cannot build signed order (market=$ORDER_MARKET genesis=${GENESIS_HASH:0:8})"
        return
    fi

    # Fresh account + registered key, then a signed resting order.
    http POST /v1/accounts '{"initial_balance_nanos":1000000000000}' none
    local acct; acct="$(echo "$HTTP_BODY" | jget account_id)"
    local kp priv pub
    kp="$("$SIGN_BIN" keygen 2>/dev/null)"
    priv="$(echo "$kp" | jget private_key_hex)"
    pub="$(echo "$kp" | jget public_key_hex)"
    http POST "/v1/accounts/$acct/keys" "{\"public_key_hex\":\"$pub\"}" none
    if ! is_2xx "$HTTP_CODE"; then
        fail "signed-order prep: key register -> $HTTP_CODE: $HTTP_BODY"; return
    fi

    local nonce osig ospk ossig obody
    nonce="$(date +%s%3N)"
    osig="$("$SIGN_BIN" order --priv "$priv" --market "$ORDER_MARKET" --nonce "$nonce" \
        --price 10000000 --qty 1000 --genesis-hash "$GENESIS_HASH" 2>/dev/null)"
    ospk="$(echo "$osig" | jget signer_pubkey_hex)"
    ossig="$(echo "$osig" | jget signature_hex)"
    if [[ -z "$ossig" ]]; then
        fail "signer produced no signature (smoke_sign order failed)"; return
    fi
    obody="$(python3 - "$ospk" "$ossig" "$ORDER_MARKET" "$nonce" <<'PY'
import sys, json
pk, sig, m, n = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
print(json.dumps({"signer_pubkey_hex": pk,
    "order": {"market_ids": [m], "payoffs": [1, 0], "limit_price_nanos": 10000000, "max_fill": 1000},
    "nonce": n, "signature_hex": sig}))
PY
)"
    http POST /v1/orders/signed "$obody" none
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget accepted)" == "true" ]]; then
        pass "signed order accepted (acct $acct, nonce $nonce)"
    else
        fail "signed order -> $HTTP_CODE: $HTTP_BODY"
    fi
}

# ── 8. Bot decisions (public) ────────────────────────────────────────────────
check_bots() {
    section "8. Bot decisions"
    http GET /v1/bots/decisions
    if is_2xx "$HTTP_CODE"; then pass "/v1/bots/decisions -> $HTTP_CODE"
    else fail "/v1/bots/decisions -> $HTTP_CODE: $HTTP_BODY"; fi
}

# ── Run ─────────────────────────────────────────────────────────────────────
echo "Sybil post-deploy smoke GATE"
echo "  API base   : $BASE"
echo "  app origin : $APP_ORIGIN"
echo "  block time : ${INTERVAL}s   service-token: $([[ -n "$SERVICE_TOKEN" ]] && echo present || echo absent)"
echo "  docker     : $([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local)"

check_liveness
check_services
check_cors
check_onboarding
check_markets
check_orders_and_fills
check_gating
check_signed_order
check_bots

section "Summary"
for r in "${RESULTS[@]}"; do
    printf '  %-4s %s\n' "${r%%|*}" "${r#*|}"
done
echo
echo "PASS=$PASSN  FAIL=$FAILN  SKIP=$SKIPN"
if [[ "$FAILN" -gt 0 ]]; then
    echo "RESULT: FAIL (deploy BLOCKED)"
    exit 1
fi
echo "RESULT: OK (promotion allowed)"
exit 0
