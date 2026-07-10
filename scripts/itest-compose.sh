#!/usr/bin/env bash
set -euo pipefail

# SYB-243 Docker Compose money-path harness.
#
# Usage:
#   scripts/itest-compose.sh             # operator/CI: runs Docker E2E
#   scripts/itest-compose.sh --dry-run   # sandbox-safe static + assertion tests

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DRY_RUN=0
case "${1:-}" in
    "") ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
        sed -n '3,8p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "unexpected arguments: ${*:2}" >&2; exit 2; }

for file in docker-compose.yml docker-compose.itest.yml \
    crates/sybil-client/examples/seed_book.rs scripts/assert-seed-book.py; do
    [[ -f "$file" ]] || { echo "missing required harness file: $file" >&2; exit 1; }
done

if [[ "$DRY_RUN" -eq 1 ]]; then
    python3 scripts/assert-seed-book.py --self-test
    printf 'dry-run: docker compose -p <isolated-project> -f docker-compose.yml -f docker-compose.itest.yml up -d --build sybil-api\n'
    printf 'dry-run: wait health -> pause -> seed_book -> resume -> assert exact block/fills/accounts -> down -v\n'
    exit 0
fi

for tool in docker curl python3 cargo; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 2; }
done
# Compose v2 plugin (docker compose) on CI, standalone v1 (docker-compose) on
# the dev box — detect whichever is present.
if docker compose version >/dev/null 2>&1; then
    COMPOSE_BIN=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_BIN=(docker-compose)
else
    echo "error: neither 'docker compose' nor 'docker-compose' is available" >&2
    exit 2
fi

PORT="${SYBIL_ITEST_PORT:-3300}"
BASE="http://127.0.0.1:$PORT"
PROJECT="sybil-itest-$(date +%s)-$$"
export COMPOSE_PROJECT_NAME="$PROJECT"
export SYBIL_ITEST_PORT="$PORT"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/itest-compose.XXXXXX")"
LOG_DIR="$ROOT/target/itest-compose"
LOG_FILE="$LOG_DIR/$PROJECT.log"
mkdir -p "$LOG_DIR"

COMPOSE=("${COMPOSE_BIN[@]}" -p "$PROJECT" -f docker-compose.yml -f docker-compose.itest.yml)
compose() { "${COMPOSE[@]}" "$@"; }

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    if [[ "$status" -ne 0 ]]; then
        compose logs --no-color >"$LOG_FILE" 2>&1 || true
        echo "compose integration failed; container logs: $LOG_FILE" >&2
    fi
    compose down -v --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$WORK"
    exit "$status"
}
trap cleanup EXIT INT TERM

pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
step() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# HTTP helper: exact expected status plus JSON parse. The body is left in the
# caller-provided file for shape/value assertions.
http_json() {
    local method=$1 path=$2 output=$3 expected=$4 body=${5:-}
    local -a args=(-sS --max-time 30 -o "$output" -w '%{http_code}' -X "$method"
        "$BASE$path" -H 'Accept: application/json')
    if [[ -n "$body" ]]; then
        args+=(-H 'Content-Type: application/json' --data "$body")
    fi
    local code
    code="$(curl "${args[@]}")"
    if [[ "$code" != "$expected" ]]; then
        echo "$method $path returned HTTP $code, expected $expected: $(cat "$output")" >&2
        return 1
    fi
    python3 -c 'import json,sys; json.load(open(sys.argv[1], encoding="utf-8"))' "$output"
}

jget() {
    python3 -c '
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
for part in sys.argv[2].split("."):
    value = value[int(part)] if isinstance(value, list) else value[part]
print("true" if value is True else "false" if value is False else value)
' "$1" "$2"
}

step "Build and start isolated sybil-api"
compose up -d --build sybil-api

ready=0
for _ in $(seq 1 90); do
    if http_json GET /v1/health "$WORK/health.json" 200 2>/dev/null \
       && [[ "$(jget "$WORK/health.json" status)" == "ok" ]] \
       && [[ "$(jget "$WORK/health.json" genesis_hash 2>/dev/null || true)" =~ ^[0-9a-f]{64}$ ]]; then
        ready=1
        break
    fi
    sleep 1
done
[[ "$ready" -eq 1 ]] || { echo "sybil-api did not become healthy with a genesis hash" >&2; exit 1; }
pass "GET /v1/health -> 200, status=ok, genesis_hash present"

step "Pause and seed the deterministic signed fixture"
http_json POST /v1/simulation/pause "$WORK/pause.json" 200
[[ "$(jget "$WORK/pause.json" status)" == "paused" ]]
pass "POST /v1/simulation/pause -> 200 paused"

cargo build -p sybil-client --example seed_book
target/debug/examples/seed_book \
    --base-url "$BASE" \
    --run-id 0 \
    --i-know-this-is-dev >"$WORK/summary.json"
python3 -c '
import json, sys
s = json.load(open(sys.argv[1], encoding="utf-8"))
assert s["schema"] == "sybil.seed_book.v1"
assert s["fixture_version"] == "SYB-247-v1:0"
assert len(s["http_steps"]) == 10
assert all(step["status"] == 200 for step in s["http_steps"])
' "$WORK/summary.json"
pass "create account -> register key -> fund -> signed orders: ten exact HTTP 200 responses"

http_json POST /v1/simulation/resume "$WORK/resume.json" 200
[[ "$(jget "$WORK/resume.json" status)" == "resumed" ]]
pass "POST /v1/simulation/resume -> 200 resumed"

YES_ACCOUNT="$(python3 -c 'import json,sys; s=json.load(open(sys.argv[1])); print(next(a["account_id"] for a in s["accounts"] if a["role"]=="buy_yes"))' "$WORK/summary.json")"
NO_ACCOUNT="$(python3 -c 'import json,sys; s=json.load(open(sys.argv[1])); print(next(a["account_id"] for a in s["accounts"] if a["role"]=="buy_no"))' "$WORK/summary.json")"

step "Wait for and assert the exact clearing block"
filled=0
for _ in $(seq 1 30); do
    http_json GET "/v1/accounts/$YES_ACCOUNT/fills?after=0.0" "$WORK/yes-fills.json" 200
    http_json GET "/v1/accounts/$NO_ACCOUNT/fills?after=0.0" "$WORK/no-fills.json" 200
    if [[ "$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))))' "$WORK/yes-fills.json")" == "1" \
       && "$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))))' "$WORK/no-fills.json")" == "1" ]]; then
        filled=1
        break
    fi
    sleep 1
done
[[ "$filled" -eq 1 ]] || { echo "fixture fills did not appear within 30 seconds" >&2; exit 1; }

BLOCK_HEIGHT="$(jget "$WORK/yes-fills.json" 0.block_height)"
http_json GET "/v1/blocks/$BLOCK_HEIGHT" "$WORK/block.json" 200
http_json GET "/v1/accounts/$YES_ACCOUNT" "$WORK/yes-account.json" 200
http_json GET "/v1/accounts/$NO_ACCOUNT" "$WORK/no-account.json" 200
pass "fill histories, exact block, and both account snapshots returned HTTP 200 JSON"

python3 scripts/assert-seed-book.py \
    --summary "$WORK/summary.json" \
    --block "$WORK/block.json" \
    --yes-account "$WORK/yes-account.json" \
    --no-account "$WORK/no-account.json" \
    --yes-fills "$WORK/yes-fills.json" \
    --no-fills "$WORK/no-fills.json"
pass "matched_volume=1000, YES/NO prices=500000000, marked balance conserved exactly"

step "Compose integration passed"
