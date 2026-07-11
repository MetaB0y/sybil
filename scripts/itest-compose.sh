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
    crates/sybil-client/examples/seed_book.rs crates/sybil-custody/src/main.rs \
    contracts/script/UnsafeAnvilEscapeSetup.s.sol scripts/assert-seed-book.py; do
    [[ -f "$file" ]] || { echo "missing required harness file: $file" >&2; exit 1; }
done

if [[ "$DRY_RUN" -eq 1 ]]; then
    python3 scripts/assert-seed-book.py --self-test
    printf 'dry-run: docker compose -p <isolated-project> -f docker-compose.yml -f docker-compose.itest.yml up -d --build sybil-api\n'
    printf 'dry-run: wait health -> seed -> snapshot/reconstruct -> unsafe Anvil fixture claim/payout -> down -v\n'
    exit 0
fi

for tool in docker curl python3 cargo anvil forge cast; do
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
    if [[ -n "${ANVIL_PID:-}" ]]; then
        kill "$ANVIL_PID" >/dev/null 2>&1 || true
        wait "$ANVIL_PID" >/dev/null 2>&1 || true
    fi
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
assert len(s["http_steps"]) == 8
assert all(step["status"] == 200 for step in s["http_steps"])
' "$WORK/summary.json"
pass "atomic account + key -> fund -> signed orders: eight exact HTTP 200 responses"

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

step "Run the anyone-can-prove custody escape fixture drill"
http_json POST /v1/simulation/pause "$WORK/escape-pause.json" 200
[[ "$(jget "$WORK/escape-pause.json" status)" == "paused" ]]

http_json GET /v1/blocks/latest "$WORK/escape-block.json" 200
ESCAPE_HEIGHT="$(jget "$WORK/escape-block.json" height)"
http_json GET "/v1/da/$ESCAPE_HEIGHT/manifest" "$WORK/escape-manifest-api.json" 200

ANVIL_PORT="${SYBIL_ITEST_ANVIL_PORT:-18545}"
ANVIL_RPC="http://127.0.0.1:$ANVIL_PORT"
ANVIL_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
ANVIL_ADMIN="0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
anvil --silent --port "$ANVIL_PORT" >"$WORK/anvil.log" 2>&1 &
ANVIL_PID=$!
for _ in $(seq 1 30); do
    cast chain-id --rpc-url "$ANVIL_RPC" >/dev/null 2>&1 && break
    sleep 1
done
cast chain-id --rpc-url "$ANVIL_RPC" >/dev/null

export PRIVATE_KEY="$ANVIL_KEY"
export ROOT_HEIGHT="$ESCAPE_HEIGHT"
export STATE_ROOT="0x$(jget "$WORK/escape-manifest-api.json" state_root)"
export BLOCK_HASH="0x$(jget "$WORK/escape-manifest-api.json" block_hash)"
export WITNESS_ROOT="0x$(jget "$WORK/escape-manifest-api.json" witness_root)"
export DA_COMMITMENT="0x$(jget "$WORK/escape-manifest-api.json" da_commitment)"
(cd contracts && forge script script/UnsafeAnvilEscapeSetup.s.sol:UnsafeAnvilEscapeSetup \
    --rpc-url "$ANVIL_RPC" --broadcast) >"$WORK/escape-setup.log"

BROADCAST="contracts/broadcast/UnsafeAnvilEscapeSetup.s.sol/31337/run-latest.json"
[[ -f "$BROADCAST" ]] || { echo "escape setup broadcast artifact missing" >&2; exit 1; }
read -r TOKEN SETTLEMENT VAULT < <(python3 - "$BROADCAST" <<'PY'
import json, sys
txs = json.load(open(sys.argv[1], encoding="utf-8"))["transactions"]
created = {tx.get("contractName"): tx.get("contractAddress") for tx in txs if tx.get("contractAddress")}
print(created["MockUSDC"], created["SybilSettlement"], created["SybilVault"])
PY
)

cargo build -p sybil-custody
target/debug/sybil-custody snapshot \
    --api-url "$BASE" \
    --account-id "$YES_ACCOUNT" \
    --rpc-url "$ANVIL_RPC" \
    --settlement "$SETTLEMENT" \
    --proof-out "$WORK/custody-proof.json" \
    --manifest-out "$WORK/custody-manifest.json" >"$WORK/custody-snapshot-result.json"
python3 -c 'import json,sys; x=json.load(open(sys.argv[1])); assert x["l1_authenticated"] is True' \
    "$WORK/custody-snapshot-result.json"
pass "custody snapshot wrote same-height own-leaf proofs + L1-authenticated DA manifest"

target/debug/sybil-custody reconstruct \
    --api-url "$BASE" \
    --height "$ESCAPE_HEIGHT" \
    --account-id "$YES_ACCOUNT" \
    --snapshot "$WORK/custody-proof.json" \
    --manifest "$WORK/custody-manifest.json" \
    --rpc-url "$ANVIL_RPC" \
    --settlement "$SETTLEMENT" >"$WORK/custody-reconstruct.json"
python3 -c 'import json,sys; x=json.load(open(sys.argv[1])); assert x["withdrawable_token_units"] > 0' \
    "$WORK/custody-reconstruct.json"
pass "custody reconstruct verified payload -> witness -> DA commitment -> L1 root and valued account"

cast rpc --rpc-url "$ANVIL_RPC" evm_increaseTime 2 >/dev/null
cast rpc --rpc-url "$ANVIL_RPC" evm_mine >/dev/null
cast send "$VAULT" "activateEscapeMode()" \
    --rpc-url "$ANVIL_RPC" --private-key "$ANVIL_KEY" >/dev/null

USER_BEFORE="$(cast call "$TOKEN" "balanceOf(address)(uint256)" "$ANVIL_ADMIN" --rpc-url "$ANVIL_RPC")"
VAULT_BEFORE="$(cast call "$TOKEN" "balanceOf(address)(uint256)" "$VAULT" --rpc-url "$ANVIL_RPC")"
USER_BEFORE="$(cast to-dec "${USER_BEFORE%% *}")"
VAULT_BEFORE="$(cast to-dec "${VAULT_BEFORE%% *}")"
P256_KEY="$(printf '%064x' 1)"
target/debug/sybil-custody escape-claim \
    --snapshot "$WORK/custody-proof.json" \
    --rpc-url "$ANVIL_RPC" \
    --settlement "$SETTLEMENT" \
    --vault "$VAULT" \
    --recipient "$ANVIL_ADMIN" \
    --p256-private-key "$P256_KEY" \
    --work-dir "$WORK/custody-work" \
    --fixture-proof \
    --submit \
    --eth-private-key "$ANVIL_KEY" >"$WORK/custody-claim.log"
USER_AFTER="$(cast call "$TOKEN" "balanceOf(address)(uint256)" "$ANVIL_ADMIN" --rpc-url "$ANVIL_RPC")"
VAULT_AFTER="$(cast call "$TOKEN" "balanceOf(address)(uint256)" "$VAULT" --rpc-url "$ANVIL_RPC")"
USER_AFTER="$(cast to-dec "${USER_AFTER%% *}")"
VAULT_AFTER="$(cast to-dec "${VAULT_AFTER%% *}")"
python3 - "$USER_BEFORE" "$USER_AFTER" "$VAULT_BEFORE" "$VAULT_AFTER" <<'PY'
import sys
user_before, user_after, vault_before, vault_after = map(int, sys.argv[1:])
assert user_after > user_before, (user_before, user_after)
assert user_after - user_before == vault_before - vault_after
PY
pass "escape activation -> fixture adapter proof -> custody calldata submission paid exact claim"

step "Compose integration passed"
