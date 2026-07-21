#!/usr/bin/env bash
# Read-only external synthetic probe for the Sybil API (SYB-250).
#
# Usage:
#   scripts/synthetic-probe.sh [--base-url URL] [--app-origin ORIGIN]
#                              [--block-interval SECONDS] [--dry-run]
#                              [--proof-lag off|warn|fail] [--proof-lag-max BLOCKS]
#                              [--prover-base URL]
#
# Environment:
#   SYBIL_SMOKE_BASE, SYBIL_SMOKE_APP_ORIGIN, SYBIL_SMOKE_INTERVAL
#   SYBIL_SMOKE_DOCKER_SSH  optional SSH target for the shared container check
#   SYBIL_COMPOSE_PROJECT   compose project (default: sybil)
#   SYBIL_SYNTHETIC_VM_URL  direct VictoriaMetrics URL; when unset, post via
#                           the local compose victoriametrics container
#   SYBIL_SMOKE_PROOF_LAG      proof-lag check mode: off|warn|fail (default: fail;
#                              the devnet mock prover keeps /proofs/latest within
#                              ~1 block of the chain head)
#   SYBIL_SMOKE_PROOF_LAG_MAX  max blocks /proofs/latest may trail
#                              /v1/blocks/latest (default: 30 = one probe period
#                              at the 10s block cadence)
#   SYBIL_SMOKE_PROVER_BASE    prover status API base URL, may embed basic-auth
#                              credentials (e.g. https://user:pass@prover.<host>);
#                              when unset, read /proofs/latest via docker exec
#                              into the compose sybil-prover container

set -uo pipefail

BASE="${SYBIL_SMOKE_BASE:-https://62-171-170-238.nip.io}"
APP_ORIGIN="${SYBIL_SMOKE_APP_ORIGIN:-https://app.62-171-170-238.nip.io}"
INTERVAL="${SYBIL_SMOKE_INTERVAL:-10}"
DOCKER_SSH="${SYBIL_SMOKE_DOCKER_SSH:-}"
COMPOSE_PROJECT="${SYBIL_COMPOSE_PROJECT:-sybil}"
VM_URL="${SYBIL_SYNTHETIC_VM_URL:-}"
PROOF_LAG_MODE="${SYBIL_SMOKE_PROOF_LAG:-fail}"
PROOF_LAG_MAX="${SYBIL_SMOKE_PROOF_LAG_MAX:-30}"
PROVER_BASE="${SYBIL_SMOKE_PROVER_BASE:-}"
DRY_RUN=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --base-url) BASE="${2:?missing URL after --base-url}"; shift 2 ;;
        --app-origin) APP_ORIGIN="${2:?missing origin after --app-origin}"; shift 2 ;;
        --block-interval) INTERVAL="${2:?missing seconds after --block-interval}"; shift 2 ;;
        --proof-lag) PROOF_LAG_MODE="${2:?missing mode after --proof-lag}"; shift 2 ;;
        --proof-lag-max) PROOF_LAG_MAX="${2:?missing blocks after --proof-lag-max}"; shift 2 ;;
        --prover-base) PROVER_BASE="${2:?missing URL after --prover-base}"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        *) echo "unknown argument: $1" >&2; usage 2 ;;
    esac
done

BASE="${BASE%/}"
APP_ORIGIN="${APP_ORIGIN%/}"
PROVER_BASE="${PROVER_BASE%/}"
[[ "$INTERVAL" =~ ^[1-9][0-9]*$ ]] || { echo "error: block interval must be a positive integer" >&2; exit 2; }
case "$PROOF_LAG_MODE" in
    off|warn|fail) ;;
    *) echo "error: proof-lag mode must be off, warn, or fail" >&2; exit 2 ;;
esac
[[ "$PROOF_LAG_MAX" =~ ^[1-9][0-9]*$ ]] || { echo "error: proof-lag max must be a positive integer" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/smoke-common.sh"

if [[ "$DRY_RUN" -eq 1 ]]; then
    cat <<EOF
dry-run: GET $BASE/v1/health and require status=ok plus a positive height and 64-hex genesis_hash
dry-run: GET $BASE/v1/blocks/latest twice, ${INTERVAL}s block interval aware, and require height advancement
dry-run: GET $BASE/v1/markets and require a nonempty JSON array
dry-run: GET $APP_ORIGIN/ and require the Sybil app shell
dry-run: GET one emitted Next.js JavaScript asset and require a nonempty JavaScript response
dry-run: measure nonzero product liquidity for active native and eligible Polymarket markets
dry-run: OPTIONS $BASE/v1/onboarding/accounts from Origin: $APP_ORIGIN and require POST CORS permission
dry-run: require /proofs/latest height within $PROOF_LAG_MAX blocks of /v1/blocks/latest (mode: $PROOF_LAG_MODE; source: ${PROVER_BASE:-docker exec into compose service sybil-prover})
dry-run: inspect compose project '$COMPOSE_PROJECT' containers ($([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local-docker)) when Docker is available
dry-run: write probe, proof-lag, MM product-liquidity, and per-service lifecycle/memory metrics to ${VM_URL:-compose victoriametrics service}
EOF
    exit 0
fi

for tool in curl python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: required tool '$tool' is unavailable" >&2; exit 2; }
done

TMP="$(mktemp -d "${TMPDIR:-/tmp}/synthetic-probe.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

prom_label_escape() {
    python3 -c 'import sys; print(sys.argv[1].replace("\\", "\\\\").replace("\n", "\\n").replace(chr(34), "\\\""))' "$1"
}

push_metric_line() {
    local line=$1 vm_container line_q

    if [[ -n "$VM_URL" ]]; then
        curl -fsS --max-time 10 --data-binary "$line" \
            "${VM_URL%/}/api/v1/import/prometheus" >/dev/null 2>&1
        return
    fi

    smoke_docker_available "$DOCKER_SSH" || return 1
    vm_container="$(smoke_docker_run "$DOCKER_SSH" \
        "docker ps -q --filter label=com.docker.compose.project=$COMPOSE_PROJECT --filter label=com.docker.compose.service=victoriametrics" \
        | head -1)"
    [[ -n "$vm_container" ]] || return 1
    printf -v line_q '%q' "$line"
    smoke_docker_run "$DOCKER_SSH" \
        "docker exec $vm_container wget -qO- --header='Content-Type: text/plain' --post-data=$line_q http://127.0.0.1:8428/api/v1/import/prometheus" \
        >/dev/null
}

push_result_metric() {
    local value=$1 instance
    instance="$(prom_label_escape "$BASE")"
    push_metric_line "sybil_synthetic_probe_failure{job=\"sybil-synthetic\",instance=\"$instance\"} $value"
}

push_proof_lag_metric() {
    local lag=$1 instance
    instance="$(prom_label_escape "$BASE")"
    push_metric_line "sybil_synthetic_proof_lag_blocks{job=\"sybil-synthetic\",instance=\"$instance\"} $lag"
}

push_proof_lag_limit_metric() {
    local limit=$1 instance
    instance="$(prom_label_escape "$BASE")"
    push_metric_line "sybil_synthetic_proof_lag_limit_blocks{job=\"sybil-synthetic\",instance=\"$instance\"} $limit"
}

push_mm_liquidity_metric() {
    local metric=$1 catalog=$2 value=$3 instance
    instance="$(prom_label_escape "$BASE")"
    push_metric_line "$metric{job=\"sybil-synthetic\",instance=\"$instance\",catalog=\"$catalog\"} $value"
}

push_container_resource_metrics() {
    local instance rows batch="" service restart_count oom_killed memory_limit started_at_seconds memory_current memory_peak service_label oom_value
    instance="$(prom_label_escape "$BASE")"
    rows="$(smoke_compose_resource_rows "$DOCKER_SSH" "$COMPOSE_PROJECT")" \
        || return 1
    [[ -n "$rows" ]] || return 1

    while read -r service restart_count oom_killed memory_limit started_at_seconds memory_current memory_peak; do
        [[ -n "$service" && "$restart_count" =~ ^[0-9]+$ ]] || return 1
        [[ "$memory_limit" =~ ^[0-9]+$ && "$started_at_seconds" =~ ^[1-9][0-9]*$ \
            && "$memory_current" =~ ^[0-9]+$ && "$memory_peak" =~ ^[0-9]+$ ]] \
            || return 1
        case "$oom_killed" in
            true) oom_value=1 ;;
            false) oom_value=0 ;;
            *) return 1 ;;
        esac
        service_label="$(prom_label_escape "$service")"
        batch+="${batch:+$'\n'}sybil_synthetic_container_restart_count{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $restart_count"
        batch+=$'\n'
        batch+="sybil_synthetic_container_oom_killed{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $oom_value"
        batch+=$'\n'
        batch+="sybil_synthetic_container_started_at_seconds{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $started_at_seconds"
        batch+=$'\n'
        batch+="sybil_synthetic_container_memory_usage_bytes{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $memory_current"
        batch+=$'\n'
        batch+="sybil_synthetic_container_memory_peak_bytes{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $memory_peak"
        batch+=$'\n'
        batch+="sybil_synthetic_container_memory_limit_bytes{job=\"sybil-synthetic\",instance=\"$instance\",service=\"$service_label\"} $memory_limit"
    done <<< "$rows"
    push_metric_line "$batch"
}

die() {
    local reason=$1
    push_result_metric 1 || true
    echo "FAIL: $reason" >&2
    exit 1
}

get_json() {
    local path=$1
    if ! HTTP_CODE="$(curl -sS --max-time 20 -o "$TMP/body" -w '%{http_code}' \
        -H 'Accept: application/json' "$BASE$path" 2>/dev/null)"; then
        HTTP_CODE=000
    fi
    HTTP_BODY="$(cat "$TMP/body" 2>/dev/null || true)"
}

get_json /v1/health
[[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ ]] || die "/v1/health returned HTTP $HTTP_CODE"
[[ "$(printf '%s' "$HTTP_BODY" | smoke_jget status)" == "ok" ]] \
    || die "/v1/health did not report status=ok"
HEALTH_HEIGHT="$(printf '%s' "$HTTP_BODY" | smoke_jget height)"
GENESIS_HASH="$(printf '%s' "$HTTP_BODY" | smoke_jget genesis_hash)"
smoke_is_committed_chain_identity "$HEALTH_HEIGHT" "$GENESIS_HASH" \
    || die "/v1/health did not expose a positive height and lowercase 64-hex genesis_hash"

get_json /v1/blocks/latest
[[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ ]] || die "/v1/blocks/latest returned HTTP $HTTP_CODE"
HEIGHT_1="$(printf '%s' "$HTTP_BODY" | smoke_jget height)"
[[ "$HEIGHT_1" =~ ^[0-9]+$ ]] || die "/v1/blocks/latest returned no numeric height"
WAIT_SECONDS=$((INTERVAL + (INTERVAL + 1) / 2))
sleep "$WAIT_SECONDS"
get_json /v1/blocks/latest
HEIGHT_2="$(printf '%s' "$HTTP_BODY" | smoke_jget height)"
[[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ && "$HEIGHT_2" =~ ^[0-9]+$ ]] \
    || die "/v1/blocks/latest second sample was invalid"
(( HEIGHT_2 > HEIGHT_1 )) || die "block height did not advance ($HEIGHT_1 -> $HEIGHT_2 in ${WAIT_SECONDS}s)"

get_json /v1/markets
[[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ ]] || die "/v1/markets returned HTTP $HTTP_CODE"
printf '%s' "$HTTP_BODY" | python3 -c \
    'import json,sys; v=json.load(sys.stdin); raise SystemExit(0 if isinstance(v,list) and len(v)>0 else 1)' \
    2>/dev/null || die "/v1/markets was empty or not a JSON array"

# A healthy local Next process does not prove that the public nginx -> Caddy
# route or the emitted static-asset path works. Exercise both read-only paths
# from the same public origin a browser uses. This also supplies a small,
# regular normal-traffic sample for the frontend memory/lifecycle envelope.
if ! APP_CODE="$(curl -sS -L --max-time 20 -o "$TMP/app.html" -w '%{http_code}' \
    "$APP_ORIGIN/" 2>/dev/null)"; then
    die "GET $APP_ORIGIN/ failed"
fi
[[ "$APP_CODE" =~ ^2[0-9][0-9]$ ]] || die "GET $APP_ORIGIN/ returned HTTP $APP_CODE"
grep -Fq '<title>Sybil</title>' "$TMP/app.html" \
    || die "GET $APP_ORIGIN/ did not return the Sybil app shell"

APP_ASSET_URL="$(python3 - "$APP_ORIGIN" "$TMP/app.html" <<'PY'
import html
import re
import sys
from pathlib import Path
from urllib.parse import urljoin

origin, path = sys.argv[1:]
document = Path(path).read_text(encoding="utf-8", errors="replace")
asset = next(
    (
        html.unescape(value)
        for value in re.findall(r'(?:src|href)="([^"]+)"', document)
        if "/_next/static/" in value and ".js" in value
    ),
    "",
)
if asset:
    print(urljoin(f"{origin}/", asset))
PY
)"
[[ -n "$APP_ASSET_URL" ]] || die "Sybil app shell exposed no Next.js JavaScript asset"
if ! APP_ASSET_META="$(curl -sS -L --max-time 20 -o /dev/null \
    -w '%{http_code}|%{content_type}|%{size_download}' "$APP_ASSET_URL" 2>/dev/null)"; then
    die "GET $APP_ASSET_URL failed"
fi
IFS='|' read -r APP_ASSET_CODE APP_ASSET_TYPE APP_ASSET_SIZE <<<"$APP_ASSET_META"
[[ "$APP_ASSET_CODE" =~ ^2[0-9][0-9]$ \
    && "$APP_ASSET_TYPE" == *javascript* \
    && "$APP_ASSET_SIZE" =~ ^[1-9][0-9]*$ ]] \
    || die "Next.js asset returned ${APP_ASSET_CODE:-unknown}/${APP_ASSET_TYPE:-unknown}/${APP_ASSET_SIZE:-0}B"

NOW_MS="$(date +%s)000"
LIQUIDITY_SUMMARY=""
for catalog in native polymarket; do
    get_json "/v1/markets/search?tags=$catalog&status=active&limit=200"
    [[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ ]] \
        || die "/v1/markets/search for $catalog returned HTTP $HTTP_CODE"
    COUNTS="$(printf '%s' "$HTTP_BODY" | python3 -c '
import json, sys

catalog = sys.argv[1]
now_ms = int(sys.argv[2])
markets = json.load(sys.stdin)
if not isinstance(markets, list):
    raise SystemExit(1)

def integer(value):
    try:
        return int(value)
    except (TypeError, ValueError):
        return None

eligible = []
for market in markets:
    if catalog == "native":
        eligible.append(market)
        continue
    reference = integer(market.get("reference_price_nanos"))
    expires = integer(market.get("reference_price_expires_at_ms"))
    if reference is not None and 10_000_000 < reference < 990_000_000 and expires is not None and expires > now_ms:
        eligible.append(market)

liquid = sum(1 for market in eligible if (integer(market.get("liquidity_avg10_nanos")) or 0) > 0)
print(len(eligible), liquid)
' "$catalog" "$NOW_MS" 2>/dev/null)" \
        || die "/v1/markets/search for $catalog was not valid market JSON"
    read -r ELIGIBLE LIQUID <<<"$COUNTS"
    [[ "$ELIGIBLE" =~ ^[1-9][0-9]*$ && "$LIQUID" =~ ^[0-9]+$ && "$LIQUID" -le "$ELIGIBLE" ]] \
        || die "$catalog liquidity coverage counts were invalid ($COUNTS)"
    push_mm_liquidity_metric sybil_synthetic_mm_eligible_markets "$catalog" "$ELIGIBLE" \
        || die "failed to publish $catalog MM eligible-market metric"
    push_mm_liquidity_metric sybil_synthetic_mm_liquid_markets "$catalog" "$LIQUID" \
        || die "failed to publish $catalog MM liquid-market metric"
    LIQUIDITY_SUMMARY="${LIQUIDITY_SUMMARY}${LIQUIDITY_SUMMARY:+, }$catalog liquidity $LIQUID/$ELIGIBLE"
done

curl -sS --max-time 20 -D "$TMP/cors-headers" -o /dev/null -X OPTIONS \
    "$BASE/v1/onboarding/accounts" \
    -H "Origin: $APP_ORIGIN" \
    -H 'Access-Control-Request-Method: POST' \
    -H 'Access-Control-Request-Headers: content-type' >/dev/null 2>&1 \
    || die "CORS preflight did not receive a response"
CORS_CODE="$(awk 'toupper($1) ~ /^HTTP/ {c=$2} END{print c}' "$TMP/cors-headers")"
CORS_ORIGIN="$(awk 'BEGIN{IGNORECASE=1} /^access-control-allow-origin:/ {sub(/^[^:]*:[ \t]*/,""); gsub(/\r/,""); print; exit}' "$TMP/cors-headers")"
CORS_METHODS="$(awk 'BEGIN{IGNORECASE=1} /^access-control-allow-methods:/ {sub(/^[^:]*:[ \t]*/,""); gsub(/\r/,""); print; exit}' "$TMP/cors-headers")"
[[ "$CORS_CODE" =~ ^2[0-9][0-9]$ ]] || die "CORS preflight returned HTTP ${CORS_CODE:-unknown}"
[[ "$CORS_ORIGIN" == "$APP_ORIGIN" ]] || die "CORS allow-origin was '${CORS_ORIGIN:-missing}', expected '$APP_ORIGIN'"
grep -qi 'POST' <<< "$CORS_METHODS" || die "CORS allow-methods did not include POST"

# ── Proof-pipeline freshness ─────────────────────────────────────────────────
# Assert that the proof-status head (sybil-prover serve API /proofs/latest,
# fed by the prover worker or the live mock prover) keeps up with
# /v1/blocks/latest. A wedged prover pipeline (e.g. the openvm pk
# bitcode-error class) is otherwise invisible to this probe: blocks keep
# advancing while nothing is being proven.

# Print the /proofs/latest body from the configured prover base URL, or via
# docker exec into the compose sybil-prover container (curl is present in
# that image; its own compose healthcheck uses it). Exits non-zero only when
# no prover container exists in the compose project; request failures print
# an empty body instead so the caller can tell the two states apart.
fetch_latest_proof_status() {
    if [[ -n "$PROVER_BASE" ]]; then
        curl -sS --max-time 20 -H 'Accept: application/json' \
            "$PROVER_BASE/proofs/latest" 2>/dev/null
        return 0
    fi
    local prover_container
    prover_container="$(smoke_docker_run "$DOCKER_SSH" \
        "docker ps -q --filter label=com.docker.compose.project=$COMPOSE_PROJECT --filter label=com.docker.compose.service=sybil-prover" \
        | head -1)"
    [[ -n "$prover_container" ]] || return 1
    smoke_docker_run "$DOCKER_SSH" \
        "docker exec $prover_container curl -sS --max-time 10 http://127.0.0.1:3002/proofs/latest" \
        || true
}

# In warn mode a violation is one loud line and the probe stays green; the
# pushed sybil_synthetic_proof_lag_blocks sample still drives vmalert.
proof_lag_violation() {
    local reason=$1
    if [[ "$PROOF_LAG_MODE" == "fail" ]]; then
        die "$reason"
    fi
    PROOF_LAG_SUMMARY="proof lag warned"
    echo "WARN: $reason (proof-lag mode: warn)" >&2
}

PROOF_LAG_SUMMARY="proof lag skipped"
if [[ "$PROOF_LAG_MODE" == "off" ]]; then
    echo "SKIP: proof-lag check disabled (proof-lag mode: off)"
elif [[ -z "$PROVER_BASE" ]] && ! smoke_docker_available "$DOCKER_SSH"; then
    echo "SKIP: proof-lag check: docker unavailable ($([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local)) and no SYBIL_SMOKE_PROVER_BASE; the prover status API is unreachable from this vantage point"
elif ! PROOF_BODY="$(fetch_latest_proof_status)"; then
    proof_lag_violation "no sybil-prover container found in compose project '$COMPOSE_PROJECT'"
elif [[ -z "$PROOF_BODY" ]]; then
    proof_lag_violation "no response from the prover status API /proofs/latest (service down or unreachable)"
else
    PROOF_HEIGHT="$(printf '%s' "$PROOF_BODY" | smoke_jget block_height)"
    if [[ ! "$PROOF_HEIGHT" =~ ^[0-9]+$ ]]; then
        proof_lag_violation "prover /proofs/latest returned no numeric block_height (body: ${PROOF_BODY:0:200}); the proof pipeline may have never started"
    else
        get_json /v1/blocks/latest
        CHAIN_HEIGHT="$(printf '%s' "$HTTP_BODY" | smoke_jget height)"
        [[ "$HTTP_CODE" =~ ^2[0-9][0-9]$ && "$CHAIN_HEIGHT" =~ ^[0-9]+$ ]] \
            || die "/v1/blocks/latest sample for the proof-lag check was invalid (HTTP $HTTP_CODE)"
        CANONICAL_ROOT=""
        CANONICAL_READY=1
        if (( PROOF_HEIGHT <= CHAIN_HEIGHT )); then
            get_json "/v1/blocks/$PROOF_HEIGHT"
            CANONICAL_ROOT="$(printf '%s' "$HTTP_BODY" | smoke_jget state_root)"
            if [[ ! "$HTTP_CODE" =~ ^2[0-9][0-9]$ || -z "$CANONICAL_ROOT" ]]; then
                CANONICAL_READY=0
                proof_lag_violation "canonical block $PROOF_HEIGHT was unavailable for proof identity binding (HTTP $HTTP_CODE)"
            fi
        else
            CANONICAL_READY=0
            proof_lag_violation "proof height $PROOF_HEIGHT is ahead of current chain height $CHAIN_HEIGHT"
        fi
        if [[ "$CANONICAL_READY" == "1" ]]; then
            PROOF_RESULT="$(printf '%s' "$PROOF_BODY" | smoke_proof_lag_result "$CHAIN_HEIGHT" "$PROOF_LAG_MAX" "$CANONICAL_ROOT" || true)"
            read -r PROOF_STATE PROOF_HEIGHT PROOF_LAG <<< "$PROOF_RESULT"
            if [[ "$PROOF_STATE" == "ERR" ]]; then
                proof_lag_violation "prover /proofs/latest is invalid (${PROOF_HEIGHT:-unknown}; body: ${PROOF_BODY:0:200}); the proof pipeline may have never started"
            else
                push_proof_lag_metric "$PROOF_LAG" || true
                push_proof_lag_limit_metric "$PROOF_LAG_MAX" || true
                if [[ "$PROOF_STATE" == "STALE" ]]; then
                    proof_lag_violation "proof height $PROOF_HEIGHT trails chain height $CHAIN_HEIGHT by $PROOF_LAG blocks (max $PROOF_LAG_MAX); the prover pipeline looks wedged"
                else
                    PROOF_LAG_SUMMARY="proof lag $PROOF_LAG<=$PROOF_LAG_MAX blocks"
                fi
            fi
        fi
    fi
fi

SERVICE_FAILURE=""
service_ok() { :; }
service_fail() { [[ -n "$SERVICE_FAILURE" ]] || SERVICE_FAILURE=$1; }
service_unavailable() { :; }
smoke_check_compose_services "$DOCKER_SSH" "$COMPOSE_PROJECT" \
    service_ok service_fail service_unavailable
[[ -z "$SERVICE_FAILURE" ]] || die "$SERVICE_FAILURE"
if smoke_docker_available "$DOCKER_SSH"; then
    push_container_resource_metrics \
        || die "failed to publish compose container lifecycle/memory metrics"
fi

if ! push_result_metric 0; then
    # The API probe succeeded. Metric-delivery failure is itself actionable and
    # must make the timer fail so journald records that alert state went stale.
    echo "FAIL: probe passed but VictoriaMetrics result delivery failed" >&2
    exit 1
fi

echo "OK: public web app, health, advancing blocks ($HEIGHT_1 -> $HEIGHT_2), markets, $LIQUIDITY_SUMMARY, CORS, $PROOF_LAG_SUMMARY, and available container checks passed"
