#!/usr/bin/env bash
# Post-deploy smoke GATE against a LIVE Sybil stack (SYB-223, hardened by SYB-240).
#
# This is the LAST deploy step: it runs against the live stack and BLOCKS
# promotion on any broken core flow. It is fail-closed — it exits non-zero if
# ANY core check FAILs. Unlike the original SYB-223 script, the core browser and
# trading flows are HARD assertions, never silent SKIPs:
#
#   * CORS preflight from the real app origin (the browser-breakage class).
#   * Deployed web shell + one Next.js static asset (the broken-web-promotion
#     class that API-only checks cannot see).
#   * Passkey onboarding: policy discovery + unauthenticated key-only account
#     allocation (or the stable lifetime-cap exhaustion response).
#   * Signed crossing: two stable operator identities MUST receive positive
#     fills on a product market, and the history projector must serve them.
#   * Account history: the API and private projector MUST agree on their query
#     contract (the stale-history-image class).
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
#                                           [--skip-mirror-readiness]
#                                           [--require-proof-freshness]
#
# Configuration (flags override env; env overrides defaults):
#   base_url / SYBIL_SMOKE_BASE          API root host
#                                        (default https://api.sybil.exchange;
#                                        the API is at the ROOT host, not api.*)
#   --service-token / SYBIL_SERVICE_TOKEN   bearer for service-gated routes
#   --app-origin / SYBIL_SMOKE_APP_ORIGIN   browser origin for the CORS check
#                                        (default https://app.sybil.exchange)
#   --block-interval / SYBIL_SMOKE_INTERVAL block time seconds (default 10)
#   SYBIL_SMOKE_STARTUP_TIMEOUT
#                                        seconds to wait for /v1/health after a
#                                        container replacement (default 60)
#   SYBIL_SMOKE_STARTUP_POLL
#                                        seconds between health probes (default 2)
#   SYBIL_SMOKE_MIRROR_TIMEOUT
#                                        seconds to wait for a referenced mirror
#                                        market after replacement (default 180)
#   SYBIL_SMOKE_MIRROR_POLL
#                                        seconds between market probes (default 5)
#   SYBIL_SMOKE_MIRROR_MAX_AGE
#                                        maximum reference-feed age in seconds
#                                        (default 180)
#   SYBIL_SMOKE_PROOF_TIMEOUT
#                                        seconds to wait for proof catch-up
#                                        after replacement (default 120)
#   SYBIL_SMOKE_PROOF_POLL
#                                        seconds between proof probes (default 5)
#   SYBIL_SMOKE_PROOF_LAG_MAX
#                                        maximum proof lag in blocks (default 30)
#   --require-signer / SYBIL_SMOKE_REQUIRE_SIGNER=1
#                                        FAIL (not SKIP) if the signed-order
#                                        signer is unavailable. Deploy recipes
#                                        always set this because they run from
#                                        a source checkout with Cargo available.
#   --skip-mirror-readiness / SYBIL_SMOKE_SKIP_MIRROR_READINESS=1
#                                        Skip the external mirror gate only for
#                                        a web-only image promotion.
#   --require-proof-freshness / SYBIL_SMOKE_REQUIRE_PROOF_FRESHNESS=1
#                                        Fail when the Compose prover status is
#                                        unavailable, malformed, or too stale.
#
#   SYBIL_SMOKE_DOCKER_SSH   run the container-health probe over this ssh target
#                            (e.g. patty) instead of local docker.
#   SYBIL_COMPOSE_PROJECT    compose project label to enumerate (default sybil).
#   SYBIL_SMOKE_SIGN_BIN     path to a prebuilt smoke_sign binary (skips cargo).
#
# Exit: 0 only if FAIL=0. Any FAIL exits 1 and blocks promotion.

set -uo pipefail

# ── Configuration ───────────────────────────────────────────────────────────
BASE="${SYBIL_SMOKE_BASE:-https://api.sybil.exchange}"
APP_ORIGIN="${SYBIL_SMOKE_APP_ORIGIN:-https://app.sybil.exchange}"
SERVICE_TOKEN="${SYBIL_SERVICE_TOKEN:-}"
INTERVAL="${SYBIL_SMOKE_INTERVAL:-10}"
STARTUP_TIMEOUT="${SYBIL_SMOKE_STARTUP_TIMEOUT:-60}"
STARTUP_POLL="${SYBIL_SMOKE_STARTUP_POLL:-2}"
MIRROR_TIMEOUT="${SYBIL_SMOKE_MIRROR_TIMEOUT:-180}"
MIRROR_POLL="${SYBIL_SMOKE_MIRROR_POLL:-5}"
MIRROR_MAX_AGE="${SYBIL_SMOKE_MIRROR_MAX_AGE:-180}"
PROOF_TIMEOUT="${SYBIL_SMOKE_PROOF_TIMEOUT:-120}"
PROOF_POLL="${SYBIL_SMOKE_PROOF_POLL:-5}"
PROOF_LAG_MAX="${SYBIL_SMOKE_PROOF_LAG_MAX:-30}"
REQUIRE_SIGNER="${SYBIL_SMOKE_REQUIRE_SIGNER:-0}"
REQUIRE_PROOF_FRESHNESS="${SYBIL_SMOKE_REQUIRE_PROOF_FRESHNESS:-0}"
SKIP_MIRROR_READINESS="${SYBIL_SMOKE_SKIP_MIRROR_READINESS:-0}"
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
        --skip-mirror-readiness) SKIP_MIRROR_READINESS=1; shift ;;
        --require-proof-freshness) REQUIRE_PROOF_FRESHNESS=1; shift ;;
        --*) echo "unknown flag: $1" >&2; usage 2 ;;
        *)
            if [[ "$BASE_SET_BY_ARG" -eq 0 ]]; then BASE="$1"; BASE_SET_BY_ARG=1; shift
            else echo "unexpected argument: $1" >&2; usage 2; fi
            ;;
    esac
done

BASE="${BASE%/}"           # strip trailing slash
APP_ORIGIN="${APP_ORIGIN%/}"

for tool in curl python3 timeout; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done

for timeout in "$STARTUP_TIMEOUT" "$MIRROR_TIMEOUT" "$PROOF_TIMEOUT"; do
    if ! [[ "$timeout" =~ ^[0-9]+$ ]]; then
        echo "error: smoke timeouts must be non-negative integers" >&2
        exit 2
    fi
done
for flag in "$SKIP_MIRROR_READINESS" "$REQUIRE_PROOF_FRESHNESS"; do
    if [[ "$flag" != "0" && "$flag" != "1" ]]; then
        echo "error: smoke skip flags must be 0 or 1" >&2
        exit 2
    fi
done
if ! [[ "$MIRROR_POLL" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: mirror poll must be a positive integer" >&2
    exit 2
fi
if ! [[ "$MIRROR_MAX_AGE" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: mirror max age must be a positive integer" >&2
    exit 2
fi
if ! [[ "$PROOF_POLL" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: proof poll must be a positive integer" >&2
    exit 2
fi
if ! [[ "$PROOF_LAG_MAX" =~ ^[0-9]+$ ]]; then
    echo "error: proof lag max must be a non-negative integer" >&2
    exit 2
fi
python3 - "$INTERVAL" "$STARTUP_POLL" <<'PY' || exit 2
import math
import sys

for name, raw in zip(("block interval", "startup poll"), sys.argv[1:]):
    try:
        value = float(raw)
    except ValueError:
        print(f"error: {name} must be a positive number", file=sys.stderr)
        raise SystemExit(1)
    if not math.isfinite(value) or value <= 0:
        print(f"error: {name} must be a positive number", file=sys.stderr)
        raise SystemExit(1)
PY

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/lib/smoke-common.sh"
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
    smoke_jget "$1"
}

# ── HTTP helper: sets HTTP_CODE and HTTP_BODY ───────────────────────────────
# usage: http METHOD PATH [BODY] [AUTH]   AUTH in none(default)|token|bad
http() {
    local method="$1" path="$2" body="${3:-}" auth="${4:-none}" max_time="${5:-30}"
    local args=(-sS -m "$max_time" -o "$TMP/body" -w '%{http_code}' -X "$method"
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
    HTTP_BODY="$(read_http_body "$TMP/body" 2>/dev/null || true)"
}

is_2xx() { smoke_is_2xx "$1"; }

# Bash variables cannot contain NUL bytes. Most smoke responses are JSON, but
# an authorized DA payload is binary; decoding that file through command
# substitution used to emit a noisy "ignored null byte" warning. Preserve text
# bodies verbatim for assertions and represent binary bodies with a safe size
# marker (the gating matrix only needs their HTTP status).
read_http_body() {
    python3 -c '
from pathlib import Path
import sys

body = Path(sys.argv[1]).read_bytes()
if b"\0" in body:
    print(f"<binary body: {len(body)} bytes>")
else:
    print(body.decode("utf-8", errors="replace"), end="")
' "$1"
}

# ── 1. Service health ───────────────────────────────────────────────────────
HEAD_HEIGHT=0
GENESIS_HASH=""
check_liveness() {
    section "1a. API liveness"

    local deadline=$((SECONDS + STARTUP_TIMEOUT)) attempts=0
    local health_height="" health_genesis=""
    while true; do
        attempts=$((attempts + 1))
        http GET /v1/health
        health_height="$(echo "$HTTP_BODY" | jget height)"
        health_genesis="$(echo "$HTTP_BODY" | jget genesis_hash)"
        if is_2xx "$HTTP_CODE" \
           && [[ "$(echo "$HTTP_BODY" | jget status)" == "ok" ]] \
           && smoke_is_committed_chain_identity "$health_height" "$health_genesis"; then
            break
        fi
        if (( SECONDS >= deadline )); then
            break
        fi
        info "/v1/health not ready ($HTTP_CODE); retrying in ${STARTUP_POLL}s..."
        sleep "$STARTUP_POLL"
    done

    GENESIS_HASH="$health_genesis"
    if is_2xx "$HTTP_CODE" \
       && [[ "$(echo "$HTTP_BODY" | jget status)" == "ok" ]] \
       && smoke_is_committed_chain_identity "$health_height" "$GENESIS_HASH"; then
        if (( attempts > 1 )); then
            info "/v1/health became ready after $attempts attempts"
        fi
        pass "/v1/health -> ok (height=$health_height, genesis=${GENESIS_HASH:0:16}...)"
    else
        fail "/v1/health did not expose a committed chain identity -> $HTTP_CODE: $HTTP_BODY"
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

check_public_block_stream() {
    section "1b. Public WebSocket replay + live handoff"

    if [[ ! "$HEAD_HEIGHT" =~ ^[0-9]+$ || "$HEAD_HEIGHT" -le 0 ]]; then
        fail "public WebSocket check needs a positive committed head height"
        return
    fi

    local ws_base ws_url ws_timeout output
    ws_base="$(python3 -c '
import sys
url = sys.argv[1]
if url.startswith("https://"):
    print("wss://" + url[8:])
elif url.startswith("http://"):
    print("ws://" + url[7:])
else:
    raise SystemExit("unsupported base URL")
' "$BASE")" || {
        fail "could not derive WebSocket URL from $BASE"
        return
    }
    ws_url="${ws_base%/}/v2/blocks/ws?from_block=$HEAD_HEIGHT"
    ws_timeout="$(python3 -c 'import sys; print(float(sys.argv[1]) * 3 + 10)' "$INTERVAL")" || {
        fail "could not derive WebSocket timeout from block interval $INTERVAL"
        return
    }

    if output="$(python3 "$SCRIPT_DIR/_ws_resume_check.py" "$ws_url" "$HEAD_HEIGHT" "$ws_timeout" 2>&1)"; then
        pass "public /v2 block stream replayed height <= $HEAD_HEIGHT and followed live"
    else
        fail "public /v2 block stream replay/live check failed: $output"
    fi
}

# Container health for every compose service. Local docker, or over ssh.
docker_run() {
    smoke_docker_run "$DOCKER_SSH" "$*"
}
check_services() {
    section "1b. Container health (compose project '$COMPOSE_PROJECT')"
    smoke_check_compose_services "$DOCKER_SSH" "$COMPOSE_PROJECT" pass fail skip
}

check_proof_freshness() {
    section "1c. Proof-pipeline freshness"
    if ! smoke_docker_available "$DOCKER_SSH"; then
        if [[ "$REQUIRE_PROOF_FRESHNESS" == "1" ]]; then
            fail "proof freshness requires Docker access to Compose project '$COMPOSE_PROJECT'"
        else
            skip "proof freshness unavailable without Docker access"
        fi
        return
    fi

    # The product topology deliberately omits the validity profile. Do not
    # spend the prover catch-up deadline polling a service that was never
    # selected; explicit validity promotions still retain the bounded retry
    # loop below and fail closed.
    if [[ "$REQUIRE_PROOF_FRESHNESS" == "0" ]]; then
        local service_rows
        service_rows="$(smoke_compose_service_rows "$DOCKER_SSH" "$COMPOSE_PROJECT")"
        if ! awk '$1 == "sybil-prover" && $3 == "running" { found = 1 } END { exit !found }' \
            <<<"$service_rows"; then
            skip "validity profile is not running; proof freshness not applicable"
            return
        fi
    fi

    local deadline=$((SECONDS + PROOF_TIMEOUT)) attempts=0
    local chain_height="" proof_body="" canonical_root="" result="ERR no-response"
    local proof_state="ERR" proof_height="" proof_lag=""
    local chain_code="000" canonical_code="000"
    local remaining request_timeout sleep_for candidate_height
    while true; do
        if (( attempts > 0 && SECONDS >= deadline )); then
            break
        fi
        attempts=$((attempts + 1))
        remaining=$((deadline - SECONDS))
        request_timeout=10
        if (( remaining > 0 && remaining < request_timeout )); then
            request_timeout=$remaining
        elif (( remaining <= 0 )); then
            request_timeout=1
        fi

        proof_body="$(smoke_compose_service_curl "$DOCKER_SSH" "$COMPOSE_PROJECT" \
            sybil-prover http://127.0.0.1:3002/proofs/latest "$request_timeout" \
            2>/dev/null || true)"
        result="ERR no-response"
        candidate_height="$(printf '%s' "$proof_body" | jget block_height)"

        remaining=$((deadline - SECONDS))
        if [[ -n "$proof_body" && "$candidate_height" =~ ^[0-9]+$ && "$remaining" -gt 0 ]]; then
            request_timeout=10
            (( remaining < request_timeout )) && request_timeout=$remaining
            http GET /v1/blocks/latest "" none "$request_timeout"
            chain_code=$HTTP_CODE
            chain_height="$(printf '%s' "$HTTP_BODY" | jget height)"
            if ! is_2xx "$chain_code" || [[ ! "$chain_height" =~ ^[0-9]+$ ]]; then
                result="ERR invalid-chain-head"
            elif (( candidate_height > chain_height )); then
                result="ERR future-block-height"
            else
                remaining=$((deadline - SECONDS))
                if (( remaining > 0 )); then
                    request_timeout=10
                    (( remaining < request_timeout )) && request_timeout=$remaining
                    http GET "/v1/blocks/$candidate_height" "" none "$request_timeout"
                    canonical_code=$HTTP_CODE
                    canonical_root="$(printf '%s' "$HTTP_BODY" | jget state_root)"
                    if is_2xx "$canonical_code" && [[ -n "$canonical_root" ]]; then
                        result="$(printf '%s' "$proof_body" | smoke_proof_lag_result \
                            "$chain_height" "$PROOF_LAG_MAX" "$canonical_root" || true)"
                    else
                        result="ERR canonical-block-unavailable"
                    fi
                else
                    result="ERR deadline-exhausted"
                fi
            fi
        elif [[ -n "$proof_body" && ! "$candidate_height" =~ ^[0-9]+$ ]]; then
            result="ERR invalid-block-height"
        elif (( remaining <= 0 )); then
            result="ERR deadline-exhausted"
        fi
        read -r proof_state proof_height proof_lag <<< "$result"
        if [[ "$proof_state" == "OK" ]]; then
            if (( attempts > 1 )); then
                info "proof pipeline became ready after $attempts attempts"
            fi
            pass "proof height $proof_height trails chain height $chain_height by $proof_lag blocks (max $PROOF_LAG_MAX)"
            return
        fi

        remaining=$((deadline - SECONDS))
        if (( remaining <= 0 )); then
            break
        fi
        sleep_for=$PROOF_POLL
        (( sleep_for > remaining )) && sleep_for=$remaining
        info "proof pipeline not ready (${result:-ERR unknown}); retrying in ${sleep_for}s..."
        sleep "$sleep_for"
    done

    local reason
    if [[ "$proof_state" == "STALE" ]]; then
        reason="proof height $proof_height trails chain height $chain_height by $proof_lag blocks (max $PROOF_LAG_MAX)"
    elif [[ "$proof_height" == "invalid-chain-head" ]]; then
        reason="/v1/blocks/latest did not expose a numeric height"
    elif [[ "$proof_height" == "future-block-height" ]]; then
        reason="proof height ${candidate_height:-unknown} is ahead of current chain height ${chain_height:-unknown}"
    elif [[ "$proof_height" == "canonical-block-unavailable" ]]; then
        reason="canonical block $candidate_height was unavailable for proof identity binding"
    elif [[ "$proof_height" == "deadline-exhausted" ]]; then
        reason="proof identity checks exhausted their deadline"
    elif [[ -z "$proof_body" ]]; then
        reason="prover /proofs/latest was unavailable or empty"
    else
        reason="prover /proofs/latest was invalid (${proof_height:-unknown}; body: ${proof_body:0:200})"
    fi
    if [[ "$REQUIRE_PROOF_FRESHNESS" == "1" ]]; then
        fail "$reason after ${PROOF_TIMEOUT}s"
    else
        skip "$reason (proof freshness not required)"
    fi
}

# ── 2a. Deployed web shell + static asset ──────────────────────────────────────
check_web_app() {
    section "2a. Deployed web app ($APP_ORIGIN)"

    local code asset_url asset_meta asset_code asset_type asset_size bundle_failed=0
    if ! code="$(curl -sS -L -m 30 -o "$TMP/app.html" -w '%{http_code}' "$APP_ORIGIN/")"; then
        fail "GET $APP_ORIGIN/ failed (deployed web app unreachable)"
        return
    fi
    if ! is_2xx "$code"; then
        fail "GET $APP_ORIGIN/ -> $code (deployed web app unavailable)"
        return
    fi
    if grep -Fq '<title>Sybil</title>' "$TMP/app.html"; then
        pass "GET $APP_ORIGIN/ -> $code with Sybil app shell"
    else
        fail "GET $APP_ORIGIN/ -> $code but the Sybil app shell is missing"
        return
    fi

    # A 200 HTML shell is not sufficient for this interactive app: a stale or
    # misrouted /_next/static path leaves users with an unhydrated page. Resolve
    # the first emitted JavaScript asset exactly as a browser would and require
    # a non-empty JavaScript response.
    asset_url="$(python3 - "$APP_ORIGIN" "$TMP/app.html" <<'PY'
import html
import re
import sys
from pathlib import Path
from urllib.parse import urljoin

origin, path = sys.argv[1:]
document = Path(path).read_text(encoding="utf-8", errors="replace")
assets = re.findall(r'(?:src|href)="([^"]+)"', document)
asset = next(
    (html.unescape(value) for value in assets
     if "/_next/static/" in value and ".js" in value),
    "",
)
if asset:
    print(urljoin(f"{origin}/", asset))
PY
)"
    if [[ -z "$asset_url" ]]; then
        fail "GET $APP_ORIGIN/ returned no Next.js JavaScript asset"
        return
    fi
    if ! asset_meta="$(curl -sS -L -m 30 -o /dev/null \
        -w '%{http_code}|%{content_type}|%{size_download}' "$asset_url")"; then
        fail "GET $asset_url failed (Next.js static asset unreachable)"
        return
    fi
    IFS='|' read -r asset_code asset_type asset_size <<< "$asset_meta"
    if is_2xx "$asset_code" \
       && [[ "$asset_type" == *javascript* ]] \
       && [[ "$asset_size" =~ ^[0-9]+$ ]] \
       && (( asset_size > 0 )); then
        pass "Next.js asset -> $asset_code ($asset_type, ${asset_size}B)"
    else
        fail "Next.js asset -> ${asset_code:-?} (${asset_type:-no content-type}, ${asset_size:-0}B)"
    fi

    # The privacy boundary moved public realtime from canonical v1 to sanitized
    # v2. Fetch every shell-referenced JS chunk and pin that the deployed web
    # bundle actually contains the v2 client path; API-only v2 health would not
    # catch an old web image that still connects to service-gated v1.
    python3 - "$APP_ORIGIN" "$TMP/app.html" >"$TMP/app-js-assets" <<'PY'
import html
import re
import sys
from pathlib import Path
from urllib.parse import urljoin

origin, path = sys.argv[1:]
document = Path(path).read_text(encoding="utf-8", errors="replace")
for value in re.findall(r'(?:src|href)="([^"]+)"', document):
    value = html.unescape(value)
    if "/_next/static/" in value and ".js" in value:
        print(urljoin(f"{origin}/", value))
PY
    : >"$TMP/app-js-bundle"
    while IFS= read -r js_url; do
        [[ -z "$js_url" ]] && continue
        if ! curl -fsS -L -m 30 "$js_url" >>"$TMP/app-js-bundle"; then
            bundle_failed=1
            break
        fi
    done <"$TMP/app-js-assets"
    if (( bundle_failed == 1 )); then
        fail "could not inspect every deployed Next.js shell chunk for realtime protocol"
    elif grep -Fq '/v2/blocks/ws' "$TMP/app-js-bundle"; then
        pass "deployed web bundle targets the public /v2 block stream"
    else
        fail "deployed web bundle does not contain /v2/blocks/ws (API/web protocol drift)"
    fi
}

# ── 2b. CORS preflight from the app origin ──────────────────────────────────
check_cors() {
    section "2b. CORS preflight (browser origin: $APP_ORIGIN)"
    local path="/v1/onboarding/accounts"
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
        fail "access-control-allow-origin='$allow' (expected '$APP_ORIGIN') — browser POST /v1/onboarding/accounts would be blocked"
    fi
    if echo "$methods" | grep -qi 'POST'; then
        pass "access-control-allow-methods includes POST ($methods)"
    else
        fail "access-control-allow-methods='$methods' does not allow POST"
    fi
}

# ── 3. Passkey onboarding (atomic create-with-initial-key) ───────────────────
# Public onboarding accepts only `initial_key`; the server assigns a fixed
# grant and enforces a durable lifetime account-id ceiling. Explicit funding
# and the unsigned first-key endpoint are service-tier only.
mint_p256_pub() {
    python3 - <<'PY'
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
}
check_onboarding() {
    section "3. Passkey onboarding (atomic create-with-initial-key)"

    local pub; pub="$(mint_p256_pub)"
    if [[ -z "$pub" ]]; then
        skip "python 'cryptography' unavailable; cannot mint a P256 key for onboarding checks"
        return
    fi

    # 3a. Policy discovery must expose the lifetime stock and fixed grant.
    http GET /v1/onboarding "" none
    local enabled capacity remaining grant
    enabled="$(echo "$HTTP_BODY" | jget enabled)"
    capacity="$(echo "$HTTP_BODY" | jget account_capacity)"
    remaining="$(echo "$HTTP_BODY" | jget accounts_remaining)"
    grant="$(echo "$HTTP_BODY" | jget grant_nanos)"
    if is_2xx "$HTTP_CODE" && [[ "$enabled" =~ ^(true|false|True|False)$ ]] \
       && [[ "$capacity" =~ ^[0-9]+$ && "$remaining" =~ ^[0-9]+$ && "$grant" =~ ^[0-9]+$ ]]; then
        pass "GET /v1/onboarding -> policy enabled=$enabled remaining=$remaining/$capacity grant_nanos=$grant"
    else
        fail "GET /v1/onboarding -> $HTTP_CODE malformed policy: $HTTP_BODY"
        return
    fi

    # 3b. A caller-supplied funding field is rejected by the key-only DTO.
    local pubb; pubb="$(mint_p256_pub)"
    http POST /v1/onboarding/accounts "{\"initial_balance_nanos\":1,\"initial_key\":{\"public_key_hex\":\"$pubb\"}}" none
    if [[ "$HTTP_CODE" == "422" ]]; then
        pass "caller-selected onboarding funding -> 422 (key-only DTO enforced)"
    else
        fail "caller-selected onboarding funding -> $HTTP_CODE (expected 422): $HTTP_BODY"
    fi

    # 3c. Key-only public allocation succeeds while stock remains, or returns
    # the stable exhaustion response after all lifetime ids are consumed.
    http POST /v1/onboarding/accounts "{\"initial_key\":{\"public_key_hex\":\"$pub\"}}" none
    local acct code balance
    acct="$(echo "$HTTP_BODY" | jget account_id)"
    code="$(echo "$HTTP_BODY" | jget code)"
    balance="$(echo "$HTTP_BODY" | jget balance_nanos)"
    if [[ "$enabled" =~ ^(true|True)$ ]] && is_2xx "$HTTP_CODE" \
       && [[ -n "$acct" && "$balance" =~ ^[0-9]+$ && "$balance" == "$grant" ]]; then
        pass "POST /v1/onboarding/accounts (no token) -> $HTTP_CODE, account_id=$acct balance_nanos=$balance"
    elif [[ "$enabled" =~ ^(true|True)$ ]] && is_2xx "$HTTP_CODE"; then
        fail "POST /v1/onboarding/accounts returned balance_nanos=${balance:-missing}; advertised grant_nanos=$grant: $HTTP_BODY"
    elif [[ "$HTTP_CODE" == "409" && "$code" == "PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED" ]]; then
        pass "exhausted public onboarding -> 409 PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED (policy sample enabled=$enabled)"
    else
        fail "POST /v1/onboarding/accounts contradicted policy enabled=$enabled -> $HTTP_CODE: $HTTP_BODY"
    fi

    # 3d. Explicitly funded service creation, no token -> 401.
    http POST /v1/accounts '{"initial_balance_nanos":"1000000000000"}' none
    if [[ "$HTTP_CODE" == "401" ]]; then
        pass "explicitly funded POST /v1/accounts (no token) -> 401 (service-tiered)"
    else
        fail "explicitly funded POST /v1/accounts (no token) -> $HTTP_CODE (expected 401): $HTTP_BODY"
    fi

    # 3e. Unsigned first-key endpoint, no token -> 401. Account existence is
    # deliberately irrelevant because service auth runs before the handler.
    local pubd; pubd="$(mint_p256_pub)"
    http POST "/v1/accounts/${acct:-0}/keys" "{\"public_key_hex\":\"$pubd\"}" none
    if [[ "$HTTP_CODE" == "401" ]]; then
        pass "unsigned first-key POST (no token) -> 401 (service-tiered)"
    else
        fail "unsigned first-key POST (no token) -> $HTTP_CODE (expected 401): $HTTP_BODY"
    fi
}

# ── 4. Markets present (needed to trade) ─────────────────────────────────────
ORDER_MARKET=""
check_markets() {
    section "4. Markets"
    local deadline=$((SECONDS + MIRROR_TIMEOUT)) attempts=0
    local counts="" ok="ERR" native=0 mirror=0 referenced=0 pick="" ref_age=""
    local market_code="000" market_body="" remaining request_timeout sleep_for
    while true; do
        if (( attempts > 0 && SECONDS >= deadline )); then
            break
        fi
        attempts=$((attempts + 1))
        remaining=$((deadline - SECONDS))
        request_timeout=30
        if (( remaining > 0 && remaining < request_timeout )); then
            request_timeout=$remaining
        elif (( remaining <= 0 )); then
            request_timeout=1
        fi
        http GET /v1/markets "" none "$request_timeout"
        market_code=$HTTP_CODE
        market_body=$HTTP_BODY
        ok="ERR"; native=0; mirror=0; referenced=0; pick=""; ref_age=""
        if is_2xx "$HTTP_CODE"; then
            counts="$(printf '%s' "$HTTP_BODY" | smoke_market_inventory 2>/dev/null || true)"
            read -r ok native mirror referenced pick <<< "$counts"
        fi

        if [[ "$SKIP_MIRROR_READINESS" == "1" && "$ok" == "OK" ]]; then
            break
        fi
        if smoke_market_inventory_is_ready "$ok" "$native" "$mirror" "$referenced"; then
            remaining=$((deadline - SECONDS))
            request_timeout=30
            if (( remaining > 0 && remaining < request_timeout )); then
                request_timeout=$remaining
            elif (( remaining <= 0 )); then
                request_timeout=1
            fi
            http GET /metrics "" none "$request_timeout"
            if is_2xx "$HTTP_CODE"; then
                ref_age="$(printf '%s' "$HTTP_BODY" | smoke_prometheus_scalar sybil_reference_prices_age_seconds 2>/dev/null || true)"
            fi
            if smoke_reference_age_is_fresh "$ref_age" "$MIRROR_MAX_AGE"; then
                break
            fi
        fi

        remaining=$((deadline - SECONDS))
        if (( remaining <= 0 )); then
            break
        fi
        sleep_for=$MIRROR_POLL
        (( sleep_for > remaining )) && sleep_for=$remaining
        info "market registry not ready (active native=$native, active mirror=$mirror, positive refs=$referenced, ref age=${ref_age:-unknown}s); retrying in ${sleep_for}s..."
        sleep "$sleep_for"
    done

    if [[ "$ok" != "OK" ]]; then
        fail "/v1/markets did not return an array -> $market_code: $market_body"
        return
    fi
    ORDER_MARKET="$pick"
    if [[ "$native" -ge 1 ]]; then pass "active native markets: $native (>=1)"
    else fail "active native markets: $native (need >=1)"; fi
    if [[ "$SKIP_MIRROR_READINESS" == "1" ]]; then
        skip "external mirror readiness is out of scope for a web-only promotion"
    elif [[ "$mirror" -lt 1 ]]; then
        fail "active mirror markets: $mirror (need >=1 after ${MIRROR_TIMEOUT}s)"
    elif [[ "$referenced" -lt 1 ]]; then
        fail "active mirror markets with positive references: $referenced (need >=1 after ${MIRROR_TIMEOUT}s)"
    elif ! smoke_reference_age_is_fresh "$ref_age" "$MIRROR_MAX_AGE"; then
        fail "reference feed age ${ref_age:-unknown}s exceeds ${MIRROR_MAX_AGE}s after ${MIRROR_TIMEOUT}s"
    else
        if (( attempts > 1 )); then
            info "mirror reference became ready after $attempts attempts"
        fi
        pass "active mirror markets: $mirror; positive references: $referenced; feed age: ${ref_age}s"
    fi
    [[ -n "$ORDER_MARKET" ]] && info "trading against market_id=$ORDER_MARKET"
}

# ── 5. Signed product-market crossing + projected fills ──────────────────────
# The exact 0.50 fixture runs before promotion in disposable Compose state.
# Live verification reuses stable, genesis-bound operator accounts and never
# creates a market. Fully crossing orders leave no resting product state.
SIGN_BIN="${SYBIL_SMOKE_SIGN_BIN:-}"
SMOKE_ACCOUNT_ID=""
SMOKE_ORDER_ID=""
HISTORY_ACCOUNT=""
SMOKE_ACCOUNT_BALANCE_NANOS=1000000000000
SMOKE_BUY_YES_PRIV=000000000000000000000000000000000000000000000000000000000000607d
SMOKE_BUY_NO_PRIV=000000000000000000000000000000000000000000000000000000000000607e
SMOKE_CANCEL_PRIV=000000000000000000000000000000000000000000000000000000000000607f

setup_signing() {
    if [[ -n "$SIGN_BIN" && -x "$SIGN_BIN" ]]; then return; fi
    local prebuilt="$REPO_ROOT/target/debug/examples/smoke_sign"
    if ! command -v cargo >/dev/null 2>&1 \
       || [[ ! -f "$REPO_ROOT/crates/sybil-client/examples/smoke_sign.rs" ]]; then
        if [[ -x "$prebuilt" ]]; then
            SIGN_BIN="$prebuilt"
        else
            SIGN_BIN=""
        fi
        return
    fi
    info "ensuring smoke_sign signing helper matches this checkout (cargo)..."
    if cargo build -q --manifest-path "$REPO_ROOT/Cargo.toml" \
        -p sybil-client --example smoke_sign 2>"$TMP/build.log"; then
        SIGN_BIN="$prebuilt"
    else
        SIGN_BIN=""
        sed 's/^/       /' "$TMP/build.log" | tail -10
    fi
}

provision_smoke_account() {
    local role="$1" priv="$2" kp pub body
    SMOKE_ACCOUNT_ID=""
    if [[ -z "$SERVICE_TOKEN" ]]; then
        fail "stable smoke account '$role' requires the service token"
        return 1
    fi
    kp="$("$SIGN_BIN" keygen --priv "$priv" 2>/dev/null)"
    pub="$(echo "$kp" | jget public_key_hex)"
    if [[ -z "$pub" ]]; then
        fail "smoke signer could not derive the '$role' public key"
        return 1
    fi
    body="$(python3 - "$role" "$pub" "$SMOKE_ACCOUNT_BALANCE_NANOS" <<'PY'
import json
import sys

role, public_key, balance = sys.argv[1], sys.argv[2], sys.argv[3]
print(json.dumps({
    "provisioning_key": f"post-deploy-smoke/{role}/v1",
    "initial_balance_nanos": balance,
    "initial_key": {
        "public_key_hex": public_key,
        "label": f"post-deploy {role}",
        "scope": "agent",
    },
}))
PY
)"
    http POST /v1/accounts "$body" token
    SMOKE_ACCOUNT_ID="$(echo "$HTTP_BODY" | jget account_id)"
    if ! is_2xx "$HTTP_CODE" || [[ -z "$SMOKE_ACCOUNT_ID" ]]; then
        fail "stable smoke account '$role' -> $HTTP_CODE: $HTTP_BODY"
        return 1
    fi
    return 0
}

submit_smoke_order() {
    local priv="$1" payoffs="$2" price="$3" quantity="$4" nonce="$5"
    local signed public_key signature body
    SMOKE_ORDER_ID=""
    signed="$("$SIGN_BIN" order --priv "$priv" --market "$ORDER_MARKET" \
        --nonce "$nonce" --price "$price" --qty "$quantity" --payoffs "$payoffs" \
        --genesis-hash "$GENESIS_HASH" 2>/dev/null)"
    public_key="$(echo "$signed" | jget signer_pubkey_hex)"
    signature="$(echo "$signed" | jget signature_hex)"
    body="$(python3 - "$public_key" "$signature" "$ORDER_MARKET" "$nonce" \
        "$price" "$quantity" "$payoffs" <<'PY'
import json
import sys

public_key, signature = sys.argv[1], sys.argv[2]
market, nonce, quantity = map(int, (sys.argv[3], sys.argv[4], sys.argv[6]))
price = sys.argv[5]
payoffs = [int(value) for value in sys.argv[7].split(",")]
print(json.dumps({
    "signer_pubkey_hex": public_key,
    "order": {
        "market_ids": [market],
        "payoffs": payoffs,
        "limit_price_nanos": price,
        "max_fill": quantity,
    },
    "time_in_force": "GTC",
    "nonce": nonce,
    "signature_hex": signature,
}))
PY
    )"
    http POST /v1/orders/signed "$body" none
    SMOKE_ORDER_ID="$(echo "$HTTP_BODY" | python3 -c \
        'import json,sys; ids=json.load(sys.stdin).get("order_ids") or []; print(ids[0] if ids else "")' \
        2>/dev/null)"
}

cancel_smoke_order() {
    local priv="$1" account="$2" order_id="$3" nonce="$4"
    local signed public_key signature body
    signed="$("$SIGN_BIN" cancel --priv "$priv" --account "$account" --order "$order_id" \
        --nonce "$nonce" --genesis-hash "$GENESIS_HASH" 2>/dev/null)" || return 1
    public_key="$(echo "$signed" | jget signer_pubkey_hex)"
    signature="$(echo "$signed" | jget signature_hex)"
    body="$(python3 - "$public_key" "$signature" "$account" "$order_id" "$nonce" <<'PY'
import json
import sys

public_key, signature = sys.argv[1], sys.argv[2]
account, order_id, nonce = map(int, sys.argv[3:6])
print(json.dumps({
    "signer_pubkey_hex": public_key,
    "account_id": account,
    "order_id": order_id,
    "nonce": nonce,
    "signature_hex": signature,
}))
PY
)"
    http POST /v1/orders/cancel/signed "$body" none
    is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget cancelled)" == "true" ]]
}

account_fill_contains_order() {
    local account="$1" order_id="$2"
    http GET "/v1/accounts/$account/fills?market_id=$ORDER_MARKET&limit=100" "" token
    is_2xx "$HTTP_CODE" && echo "$HTTP_BODY" | python3 -c '
import json
import sys

order_id = int(sys.argv[1])
page = json.load(sys.stdin)
matches = [
    fill for fill in page.get("fills", [])
    if fill.get("order_id") == order_id and int(fill.get("fill_qty") or 0) > 0
]
raise SystemExit(0 if matches else 1)
' "$order_id" 2>/dev/null
}

check_orders_and_fills() {
    section "5. Signed crossing + history-projected fills"
    setup_signing
    if [[ -z "$SIGN_BIN" ]]; then
        if [[ "$REQUIRE_SIGNER" == "1" ]]; then
            fail "signer unavailable but --require-signer set"
        else
            skip "signer unavailable; live crossing not run"
        fi
        return
    fi
    if [[ -z "$ORDER_MARKET" || -z "$GENESIS_HASH" || -z "$SERVICE_TOKEN" ]]; then
        fail "live crossing requires market, genesis, and service token"
        return
    fi

    http GET /v1/activity/overview
    local before
    before="$(echo "$HTTP_BODY" | jget all_time.execution_quality.trader_orders_first_filled)"
    [[ "$before" =~ ^[0-9]+$ ]] || before=0

    provision_smoke_account crossing-buy-yes "$SMOKE_BUY_YES_PRIV" || return
    local yes_account="$SMOKE_ACCOUNT_ID"
    HISTORY_ACCOUNT="$yes_account"
    provision_smoke_account crossing-buy-no "$SMOKE_BUY_NO_PRIV" || return
    local no_account="$SMOKE_ACCOUNT_ID"

    local nonce yes_order no_order
    nonce="$(date +%s%3N)"
    submit_smoke_order "$SMOKE_BUY_YES_PRIV" 1,0 990000000 1000 "$nonce"
    yes_order="$SMOKE_ORDER_ID"
    if [[ -z "$yes_order" ]]; then
        fail "signed BuyYes crossing order was not accepted: $HTTP_CODE $HTTP_BODY"
        return
    fi
    submit_smoke_order "$SMOKE_BUY_NO_PRIV" 0,1 990000000 1000 "$nonce"
    no_order="$SMOKE_ORDER_ID"
    if [[ -z "$no_order" ]]; then
        cancel_smoke_order "$SMOKE_BUY_YES_PRIV" "$yes_account" "$yes_order" "$((nonce + 1))" || true
        fail "signed BuyNo crossing order was not accepted: $HTTP_CODE $HTTP_BODY"
        return
    fi
    pass "signed crossing orders accepted on product market $ORDER_MARKET (orders $yes_order/$no_order)"

    local deadline wait_seconds yes_filled=0 no_filled=0
    wait_seconds="$(python3 - "$INTERVAL" <<'PY'
import math
import sys

print(math.ceil(float(sys.argv[1]) * 6 + 10))
PY
)"
    deadline=$((SECONDS + wait_seconds))
    while (( SECONDS < deadline )); do
        account_fill_contains_order "$yes_account" "$yes_order" && yes_filled=1
        account_fill_contains_order "$no_account" "$no_order" && no_filled=1
        [[ "$yes_filled" -eq 1 && "$no_filled" -eq 1 ]] && break
        sleep 2
    done

    if [[ "$yes_filled" -eq 1 && "$no_filled" -eq 1 ]]; then
        pass "both signed product orders have positive history-projected fills"
    else
        cancel_smoke_order "$SMOKE_BUY_YES_PRIV" "$yes_account" "$yes_order" "$((nonce + 1))" || true
        cancel_smoke_order "$SMOKE_BUY_NO_PRIV" "$no_account" "$no_order" "$((nonce + 1))" || true
        fail "signed crossing did not produce both fills (BuyYes=$yes_filled BuyNo=$no_filled)"
        return
    fi

    http GET /v1/activity/overview
    local after
    after="$(echo "$HTTP_BODY" | jget all_time.execution_quality.trader_orders_first_filled)"
    if [[ "$after" =~ ^[0-9]+$ && "$after" -ge $((before + 2)) ]]; then
        pass "trader execution metric credited both first fills ($before -> $after)"
    else
        fail "trader execution metric did not credit both fills ($before -> ${after:-missing})"
    fi

    local pending=0 account
    for account in "$yes_account" "$no_account"; do
        http GET "/v1/accounts/$account/orders" "" token
        if ! is_2xx "$HTTP_CODE"; then
            pending=1
        elif ! echo "$HTTP_BODY" | python3 -c '
import json
import sys

order_ids = {int(value) for value in sys.argv[1:]}
orders = json.load(sys.stdin)
raise SystemExit(0 if all(order.get("order_id") not in order_ids for order in orders) else 1)
' "$yes_order" "$no_order" 2>/dev/null; then
            pending=1
        fi
    done
    if [[ "$pending" -eq 0 ]]; then
        pass "signed crossing left no active smoke orders"
    else
        fail "signed crossing left a smoke order resting"
    fi
}

# ── 6. Service-token gating matrix ───────────────────────────────────────────
check_gating() {
    section "6. Service-token gating matrix"

    # Create a real service-owned account when credentials are available (fund
    # requires an existing account). Without a token, use an arbitrary id: the
    # no-token/wrong-token assertions are evaluated before handler lookup.
    local acct=""
    if [[ -n "$SERVICE_TOKEN" ]]; then
        http POST /v1/accounts \
            '{"provisioning_key":"post-deploy-smoke/gating/v1","initial_balance_nanos":"1000000000"}' token
        acct="$(echo "$HTTP_BODY" | jget account_id)"
    fi
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
        body=""; [[ "$method" == "POST" ]] && body='{"amount_nanos":"1000"}'

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
        http POST "/v1/accounts/$acct/fund" '{"amount_nanos":"1000"}' bad
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

# ── 7. Signed cancel lifecycle + reservation release ─────────────────────────
# Exercises the full client cancel path the web app uses: place a deep
# out-of-market resting order (holds a balance reservation), cancel it with a
# signed request, and assert it disappears AND the reservation is released
# (available balance restored). Guards the SYB reservation-accounting path.
check_signed_cancel_lifecycle() {
    section "7. Signed cancel lifecycle + reservation release"
    setup_signing
    if [[ -z "$SIGN_BIN" ]]; then
        if [[ "$REQUIRE_SIGNER" == "1" ]]; then
            fail "signer unavailable but --require-signer set (build smoke_sign in the deploy checkout)"
        else
            skip "signer (smoke_sign) unavailable; cancel-lifecycle check skipped"
        fi
        return
    fi
    if [[ -z "$ORDER_MARKET" || -z "$GENESIS_HASH" ]]; then
        fail "cannot run cancel lifecycle (market=$ORDER_MARKET genesis=${GENESIS_HASH:0:8})"
        return
    fi

    provision_smoke_account cancellation "$SMOKE_CANCEL_PRIV" || return
    local priv="$SMOKE_CANCEL_PRIV" acct="$SMOKE_ACCOUNT_ID"

    # Deep out-of-market resting BuyYes at $0.01 so it never crosses (stays cancellable).
    local nonce osig ospk ossig obody oid
    nonce="$(date +%s%3N)"
    osig="$("$SIGN_BIN" order --priv "$priv" --market "$ORDER_MARKET" --nonce "$nonce" \
        --price 10000000 --qty 1000 --genesis-hash "$GENESIS_HASH" 2>/dev/null)"
    ospk="$(echo "$osig" | jget signer_pubkey_hex)"
    ossig="$(echo "$osig" | jget signature_hex)"
    obody="$(python3 - "$ospk" "$ossig" "$ORDER_MARKET" "$nonce" <<'PY'
import sys, json
pk, sig, m, n = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
print(json.dumps({"signer_pubkey_hex": pk,
    "order": {"market_ids": [m], "payoffs": [1, 0], "limit_price_nanos": "10000000", "max_fill": 1000},
    "nonce": n, "signature_hex": sig}))
PY
)"
    http POST /v1/orders/signed "$obody" none
    oid="$(echo "$HTTP_BODY" | python3 -c 'import sys,json; ids=json.load(sys.stdin).get("order_ids") or []; print(ids[0] if ids else "")' 2>/dev/null)"
    if [[ -z "$oid" ]]; then
        fail "cancel-lifecycle: order not accepted -> $HTTP_CODE: $HTTP_BODY"; return
    fi
    pass "cancel-lifecycle: signed order accepted (acct $acct, order $oid)"

    # Order visible + reservation held. Visibility is eventually-consistent with
    # block production (~10s), so poll rather than assume a single-shot read
    # lands after the placing block commits.
    local seen=no i
    for i in 1 2 3 4 5 6; do
        http GET "/v1/accounts/$acct/orders" "" token
        if echo "$HTTP_BODY" | python3 -c "import sys,json; sys.exit(0 if any(o.get('order_id')==$oid for o in json.load(sys.stdin)) else 1)" 2>/dev/null; then
            seen=yes; break
        fi
        sleep "$INTERVAL"
    done
    if [[ "$seen" == "yes" ]]; then
        pass "cancel-lifecycle: resting order visible in account orders"
    else
        fail "cancel-lifecycle: order $oid not visible after placement"; return
    fi
    http GET "/v1/accounts/$acct" "" token
    local reserved_held; reserved_held="$(echo "$HTTP_BODY" | jget reserved_balance_nanos)"
    if [[ -n "$reserved_held" && "$reserved_held" != "0" ]]; then
        pass "cancel-lifecycle: reservation held (reserved=$reserved_held)"
    else
        fail "cancel-lifecycle: expected non-zero reservation after resting order, got '$reserved_held'"; return
    fi

    # Signed cancel.
    local cnonce csig cspk cssig cbody
    cnonce="$((nonce + 1))"
    csig="$("$SIGN_BIN" cancel --priv "$priv" --account "$acct" --order "$oid" --nonce "$cnonce" --genesis-hash "$GENESIS_HASH" 2>/dev/null)"
    cspk="$(echo "$csig" | jget signer_pubkey_hex)"
    cssig="$(echo "$csig" | jget signature_hex)"
    cbody="$(python3 - "$cspk" "$cssig" "$acct" "$oid" "$cnonce" <<'PY'
import sys, json
pk, sig, a, o, n = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
print(json.dumps({"signer_pubkey_hex": pk, "account_id": a, "order_id": o, "nonce": n, "signature_hex": sig}))
PY
)"
    http POST /v1/orders/cancel/signed "$cbody" none
    if is_2xx "$HTTP_CODE" && [[ "$(echo "$HTTP_BODY" | jget cancelled)" == "true" ]]; then
        pass "cancel-lifecycle: signed cancel accepted"
    else
        fail "cancel-lifecycle: signed cancel -> $HTTP_CODE: $HTTP_BODY"; return
    fi

    # Order gone + reservation released.
    local gone=no i
    for i in 1 2 3 4 5 6; do
        http GET "/v1/accounts/$acct/orders" "" token
        if echo "$HTTP_BODY" | python3 -c "import sys,json; sys.exit(0 if all(o.get('order_id')!=$oid for o in json.load(sys.stdin)) else 1)" 2>/dev/null; then
            gone=yes; break
        fi
        sleep "$INTERVAL"
    done
    if [[ "$gone" == "yes" ]]; then
        pass "cancel-lifecycle: order removed after cancel"
    else
        fail "cancel-lifecycle: order $oid still present after cancel"; return
    fi
    http GET "/v1/accounts/$acct" "" token
    local reserved_after; reserved_after="$(echo "$HTTP_BODY" | jget reserved_balance_nanos)"
    if [[ "$reserved_after" == "0" ]]; then
        pass "cancel-lifecycle: reservation released (reserved=0 after cancel)"
    else
        fail "cancel-lifecycle: reservation not released, reserved=$reserved_after"
    fi
}

# ── 7c. API-to-history query contract ───────────────────────────────────────
check_history_query_contract() {
    section "7c. API-to-history query contract"
    if [[ -z "$SERVICE_TOKEN" ]]; then
        skip "service token unavailable; account history contract cannot be authenticated"
        return
    fi
    if [[ -z "$HISTORY_ACCOUNT" ]]; then
        fail "no smoke account available for the account history contract"
        return
    fi

    http GET "/v1/accounts/$HISTORY_ACCOUNT/fills?limit=1" "" token
    local scope
    scope="$(echo "$HTTP_BODY" | jget history_scope)"
    if is_2xx "$HTTP_CODE" \
       && [[ "$scope" == "remote" ]] \
       && echo "$HTTP_BODY" | python3 -c \
            'import json,sys; value=json.load(sys.stdin); raise SystemExit(0 if isinstance(value.get("fills"), list) else 1)' \
            2>/dev/null; then
        pass "account fills query reached the remote history projector"
    else
        fail "account fills history contract -> $HTTP_CODE (scope=${scope:-missing}): $HTTP_BODY"
    fi
}

# ── 8. Bot decisions (public) ────────────────────────────────────────────────
check_bots() {
    section "8. Bot decisions"
    http GET /v1/bots/decisions
    if ! is_2xx "$HTTP_CODE"; then
        fail "/v1/bots/decisions -> $HTTP_CODE: $HTTP_BODY"; return
    fi
    pass "/v1/bots/decisions -> $HTTP_CODE"
    # HTTP 200 alone is not enough: the arena decisions DB can be present but
    # unreadable (e.g. a column-type mismatch), which returns 200 with
    # db_available=false + an error and silently empties the arena view.
    local db_ok err
    db_ok="$(echo "$HTTP_BODY" | jget db_available)"
    err="$(echo "$HTTP_BODY" | jget error)"
    if [[ "$db_ok" == "false" || "$db_ok" == "False" ]]; then
        fail "arena decisions DB unreadable (db_available=false): ${err:-unknown}"
    else
        pass "arena decisions DB readable (db_available=$db_ok)"
    fi
}

# ── Run ─────────────────────────────────────────────────────────────────────
if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
    return 0
fi

echo "Sybil post-deploy smoke GATE"
echo "  API base   : $BASE"
echo "  app origin : $APP_ORIGIN"
echo "  block time : ${INTERVAL}s   service-token: $([[ -n "$SERVICE_TOKEN" ]] && echo present || echo absent)"
echo "  docker     : $([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local)"
echo "  fill gate  : signed crossing on an existing product market"
echo "  mirror gate: $([[ "$SKIP_MIRROR_READINESS" == "1" ]] && echo web-only-skip || echo required)"
echo "  proof gate : $([[ "$REQUIRE_PROOF_FRESHNESS" == "1" ]] && echo "required (lag <= $PROOF_LAG_MAX)" || echo optional)"

check_liveness
check_public_block_stream
check_services
check_proof_freshness
check_web_app
check_cors
check_onboarding
check_markets
check_orders_and_fills
check_gating
check_signed_cancel_lifecycle
check_history_query_contract
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
