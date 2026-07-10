#!/usr/bin/env bash
# Read-only external synthetic probe for the Sybil API (SYB-250).
#
# Usage:
#   scripts/synthetic-probe.sh [--base-url URL] [--app-origin ORIGIN]
#                              [--block-interval SECONDS] [--dry-run]
#
# Environment:
#   SYBIL_SMOKE_BASE, SYBIL_SMOKE_APP_ORIGIN, SYBIL_SMOKE_INTERVAL
#   SYBIL_SMOKE_DOCKER_SSH  optional SSH target for the shared container check
#   SYBIL_COMPOSE_PROJECT   compose project (default: sybil)
#   SYBIL_SYNTHETIC_VM_URL  direct VictoriaMetrics URL; when unset, post via
#                           the local compose victoriametrics container

set -uo pipefail

BASE="${SYBIL_SMOKE_BASE:-https://172-104-31-54.nip.io}"
APP_ORIGIN="${SYBIL_SMOKE_APP_ORIGIN:-https://app.172-104-31-54.nip.io}"
INTERVAL="${SYBIL_SMOKE_INTERVAL:-10}"
DOCKER_SSH="${SYBIL_SMOKE_DOCKER_SSH:-}"
COMPOSE_PROJECT="${SYBIL_COMPOSE_PROJECT:-sybil}"
VM_URL="${SYBIL_SYNTHETIC_VM_URL:-}"
DRY_RUN=0

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --base-url) BASE="${2:?missing URL after --base-url}"; shift 2 ;;
        --app-origin) APP_ORIGIN="${2:?missing origin after --app-origin}"; shift 2 ;;
        --block-interval) INTERVAL="${2:?missing seconds after --block-interval}"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        *) echo "unknown argument: $1" >&2; usage 2 ;;
    esac
done

BASE="${BASE%/}"
APP_ORIGIN="${APP_ORIGIN%/}"
[[ "$INTERVAL" =~ ^[1-9][0-9]*$ ]] || { echo "error: block interval must be a positive integer" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/smoke-common.sh"

if [[ "$DRY_RUN" -eq 1 ]]; then
    cat <<EOF
dry-run: GET $BASE/v1/health and require status=ok
dry-run: GET $BASE/v1/blocks/latest twice, ${INTERVAL}s block interval aware, and require height advancement
dry-run: GET $BASE/v1/markets and require a nonempty JSON array
dry-run: OPTIONS $BASE/v1/accounts from Origin: $APP_ORIGIN and require POST CORS permission
dry-run: inspect compose project '$COMPOSE_PROJECT' containers ($([[ -n "$DOCKER_SSH" ]] && echo "ssh $DOCKER_SSH" || echo local-docker)) when Docker is available
dry-run: write sybil_synthetic_probe_failure=0 or 1 to ${VM_URL:-compose victoriametrics service}
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

push_result_metric() {
    local value=$1 instance line vm_container
    instance="$(prom_label_escape "$BASE")"
    line="sybil_synthetic_probe_failure{job=\"sybil-synthetic\",instance=\"$instance\"} $value"

    if [[ -n "$VM_URL" ]]; then
        curl -fsS --max-time 10 --data-binary "$line" \
            "${VM_URL%/}/api/v1/import/prometheus" >/dev/null 2>&1
        return
    fi

    command -v docker >/dev/null 2>&1 || return 1
    vm_container="$(docker ps -q \
        --filter "label=com.docker.compose.project=$COMPOSE_PROJECT" \
        --filter 'label=com.docker.compose.service=victoriametrics' | head -1)"
    [[ -n "$vm_container" ]] || return 1
    docker exec "$vm_container" wget -qO- \
        --header='Content-Type: text/plain' --post-data="$line" \
        'http://127.0.0.1:8428/api/v1/import/prometheus' >/dev/null 2>&1
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

curl -sS --max-time 20 -D "$TMP/cors-headers" -o /dev/null -X OPTIONS \
    "$BASE/v1/accounts" \
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

SERVICE_FAILURE=""
service_ok() { :; }
service_fail() { [[ -n "$SERVICE_FAILURE" ]] || SERVICE_FAILURE=$1; }
service_unavailable() { :; }
smoke_check_compose_services "$DOCKER_SSH" "$COMPOSE_PROJECT" \
    service_ok service_fail service_unavailable
[[ -z "$SERVICE_FAILURE" ]] || die "$SERVICE_FAILURE"

if ! push_result_metric 0; then
    # The API probe succeeded. Metric-delivery failure is itself actionable and
    # must make the timer fail so journald records that alert state went stale.
    echo "FAIL: probe passed but VictoriaMetrics result delivery failed" >&2
    exit 1
fi

echo "OK: health, advancing blocks ($HEIGHT_1 -> $HEIGHT_2), markets, CORS, and available container checks passed"
