#!/usr/bin/env bash
set -euo pipefail

# SYB-243 Docker Compose money-path harness.
#
# Usage:
#   scripts/itest-compose.sh             # operator/CI: no-proving Docker E2E
#   scripts/itest-compose.sh --with-escape # opt into the custody proof drill
#   scripts/itest-compose.sh --dry-run   # sandbox-safe static + assertion tests

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DRY_RUN=0
SKIP_ESCAPE=1
case "${1:-}" in
    "") ;;
    --with-escape) SKIP_ESCAPE=0 ;;
    --dry-run) DRY_RUN=1 ;;
    --skip-escape) SKIP_ESCAPE=1 ;; # explicit no-proving alias
    -h|--help)
        sed -n '3,8p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "unexpected arguments: ${*:2}" >&2; exit 2; }

for file in docker-compose.yml docker-compose.itest.yml \
    crates/sybil-client/examples/seed_book.rs crates/sybil-client/examples/smoke_sign.rs \
    contracts/script/UnsafeSepoliaMockSetup.s.sol scripts/assert-seed-book.py \
    scripts/deploy-sepolia-mock-l1.sh scripts/relay-sepolia-mock-withdrawals.sh; do
    [[ -f "$file" ]] || { echo "missing required harness file: $file" >&2; exit 1; }
done
if [[ "$SKIP_ESCAPE" -eq 0 ]]; then
    for file in crates/sybil-custody/src/main.rs \
        contracts/script/UnsafeAnvilEscapeSetup.s.sol; do
        [[ -f "$file" ]] || { echo "missing required escape harness file: $file" >&2; exit 1; }
    done
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    python3 scripts/assert-seed-book.py --self-test
    printf 'dry-run: docker compose -p <isolated-project> -f docker-compose.yml -f docker-compose.itest.yml up -d --build sybil-history sybil-api\n'
    printf 'dry-run: Sepolia-chain Anvil mock deploy -> API domain boot -> seed -> optional escape -> deposit/index/relay/queue/finalize -> down -v\n'
    exit 0
fi

for tool in docker curl jq python3 cargo anvil forge cast; do
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

step "Deploy the chain-bound UNSAFE mock bridge before API startup"
ANVIL_PORT="${SYBIL_ITEST_ANVIL_PORT:-18545}"
ANVIL_RPC="http://127.0.0.1:$ANVIL_PORT"
ANVIL_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
ANVIL_ADMIN="0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
anvil --silent --chain-id 11155111 --port "$ANVIL_PORT" >"$WORK/anvil.log" 2>&1 &
ANVIL_PID=$!
for _ in $(seq 1 30); do
    cast chain-id --rpc-url "$ANVIL_RPC" >/dev/null 2>&1 && break
    sleep 1
done
[[ "$(cast chain-id --rpc-url "$ANVIL_RPC")" == "11155111" ]]

export PRIVATE_KEY="$ANVIL_KEY"
export SEPOLIA_RPC_URL="$ANVIL_RPC"
export CONFIRM_UNSAFE_SEPOLIA_MOCK=I_UNDERSTAND_PROOFS_ARE_NOT_VERIFIED
export CONFIRM_UNSAFE_SEPOLIA_MOCK_RELAY=I_UNDERSTAND_WITHDRAWALS_ARE_NOT_PROOF_VERIFIED
export SYBIL_L1_DEPLOYMENT_MANIFEST="$WORK/sepolia-mock-l1.json"
./scripts/deploy-sepolia-mock-l1.sh >"$WORK/bridge-setup.log"

BRIDGE_TOKEN="$(jq -er '.contracts.token.address' "$SYBIL_L1_DEPLOYMENT_MANIFEST")"
BRIDGE_SETTLEMENT="$(jq -er '.contracts.settlement.address' "$SYBIL_L1_DEPLOYMENT_MANIFEST")"
BRIDGE_VAULT="$(jq -er '.contracts.vault.address' "$SYBIL_L1_DEPLOYMENT_MANIFEST")"
export SYBIL_BRIDGE_CHAIN_ID=11155111
export SYBIL_BRIDGE_VAULT_ADDRESS="$BRIDGE_VAULT"
export SYBIL_BRIDGE_TOKEN_ADDRESS="$BRIDGE_TOKEN"
pass "Sepolia-only adapters, mintable token, settlement, vault, and manifest validated"

step "Build and start isolated sybil-api + history projector"
compose up -d --build sybil-history sybil-api

ready=0
for _ in $(seq 1 90); do
    if compose exec -T sybil-history curl -fsS http://127.0.0.1:3003/healthz \
           >/dev/null 2>&1 \
       && http_json GET /v1/health "$WORK/health.json" 200 2>/dev/null \
       && [[ "$(jget "$WORK/health.json" status)" == "ok" ]] \
       && [[ "$(jget "$WORK/health.json" genesis_hash 2>/dev/null || true)" =~ ^[0-9a-f]{64}$ ]]; then
        ready=1
        break
    fi
    sleep 1
done
[[ "$ready" -eq 1 ]] || { echo "sybil-api/history did not become healthy with a genesis hash" >&2; exit 1; }
pass "history health + GET /v1/health -> 200, status=ok, genesis_hash present"

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
    if [[ "$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["fills"]))' "$WORK/yes-fills.json")" == "1" \
       && "$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["fills"]))' "$WORK/no-fills.json")" == "1" ]]; then
        filled=1
        break
    fi
    sleep 1
done
[[ "$filled" -eq 1 ]] || { echo "fixture fills did not appear within 30 seconds" >&2; exit 1; }

BLOCK_HEIGHT="$(jget "$WORK/yes-fills.json" fills.0.block_height)"
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

if [[ "$SKIP_ESCAPE" -eq 0 ]]; then
step "Run the anyone-can-prove custody escape fixture drill"
http_json POST /v1/simulation/pause "$WORK/escape-pause.json" 200
[[ "$(jget "$WORK/escape-pause.json" status)" == "paused" ]]

http_json GET /v1/blocks/latest "$WORK/escape-block.json" 200
ESCAPE_HEIGHT="$(jget "$WORK/escape-block.json" height)"
http_json GET "/v1/da/$ESCAPE_HEIGHT/manifest" "$WORK/escape-manifest-api.json" 200

export ROOT_HEIGHT="$ESCAPE_HEIGHT"
export STATE_ROOT="0x$(jget "$WORK/escape-manifest-api.json" state_root)"
export BLOCK_HASH="0x$(jget "$WORK/escape-manifest-api.json" block_hash)"
export WITNESS_ROOT="0x$(jget "$WORK/escape-manifest-api.json" witness_root)"
export DA_COMMITMENT="0x$(jget "$WORK/escape-manifest-api.json" da_commitment)"
(cd contracts && forge script script/UnsafeAnvilEscapeSetup.s.sol:UnsafeAnvilEscapeSetup \
    --rpc-url "$ANVIL_RPC" --broadcast) >"$WORK/escape-setup.log"

BROADCAST="contracts/broadcast/UnsafeAnvilEscapeSetup.s.sol/11155111/run-latest.json"
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
else
step "Skip custody escape proof drill in the default no-proving profile"
http_json POST /v1/simulation/pause "$WORK/bridge-initial-pause.json" 200
[[ "$(jget "$WORK/bridge-initial-pause.json" status)" == "paused" ]]
fi

step "Run the UNSAFE Sepolia-mock normal bridge round trip"
echo "  accept-all adapters validate public-testnet plumbing, not withdrawal proof soundness"

cargo build -p sybil-client --example smoke_sign
cargo build -p sybil-l1-indexer
target/debug/examples/smoke_sign keygen >"$WORK/bridge-key.json"
BRIDGE_PRIVATE_KEY="$(jget "$WORK/bridge-key.json" private_key_hex)"
BRIDGE_PUBLIC_KEY="$(jget "$WORK/bridge-key.json" public_key_hex)"
BRIDGE_ACCOUNT_BODY="$(python3 - "$BRIDGE_PUBLIC_KEY" <<'PY'
import json, sys
print(json.dumps({
    "initial_balance_nanos": 0,
    "initial_key": {"public_key_hex": sys.argv[1], "auth_scheme": "raw_p256"},
}))
PY
)"
http_json POST /v1/accounts "$WORK/bridge-account.json" 200 "$BRIDGE_ACCOUNT_BODY"
BRIDGE_ACCOUNT="$(jget "$WORK/bridge-account.json" account_id)"
http_json GET "/v1/accounts/$BRIDGE_ACCOUNT/bridge-key" "$WORK/bridge-account-key.json" 200
BRIDGE_ACCOUNT_KEY="$(jget "$WORK/bridge-account-key.json" sybil_account_key_hex)"

BRIDGE_DEPOSIT_UNITS=5000000
cast send "$BRIDGE_TOKEN" "approve(address,uint256)" \
    "$BRIDGE_VAULT" "$BRIDGE_DEPOSIT_UNITS" \
    --rpc-url "$ANVIL_RPC" --private-key "$ANVIL_KEY" >/dev/null
cast send "$BRIDGE_VAULT" "deposit(uint256,bytes32)" \
    "$BRIDGE_DEPOSIT_UNITS" "0x$BRIDGE_ACCOUNT_KEY" \
    --rpc-url "$ANVIL_RPC" --private-key "$ANVIL_KEY" >/dev/null

BRIDGE_CURSOR="$WORK/bridge-indexer-cursor.json"
run_bridge_indexer() {
    target/debug/sybil-l1-indexer \
        --rpc-urls "$ANVIL_RPC" \
        --rpc-ids local-anvil \
        --trust-mode unsafe-single-dev \
        --sybil-api-url "$BASE" \
        --vault-address "$BRIDGE_VAULT" \
        --chain-id 11155111 \
        --start-block 0 \
        --confirmations 0 \
        --min-confirmations 0 \
        --cursor-path "$BRIDGE_CURSOR" \
        --once
}
run_bridge_indexer 2>&1 | tee "$WORK/bridge-indexer-deposit.log"
python3 - "$BRIDGE_CURSOR" <<'PY'
import json, sys
state = json.load(open(sys.argv[1], encoding="utf-8"))
assert state["schema_version"] == 3, state
assert state["checkpoint"]["block_number"] + 1 == state["next_from"], state
assert len(state["checkpoint"]["block_hash_hex"]) == 64, state
assert state["source_tip"]["block_number"] >= state["checkpoint"]["block_number"], state
assert len(state["source_tip"]["block_hash_hex"]) == 64, state
assert state["source_identity"] == {
    "trust_mode": "unsafe-single-dev",
    "provider_ids": ["local-anvil"],
}, state
assert "integrity_incident" not in state, state
PY
pass "L1 indexer persisted a deployment-bound canonical block checkpoint"
http_json GET "/v1/accounts/$BRIDGE_ACCOUNT" "$WORK/bridge-funded-account.json" 200
http_json GET /v1/bridge/status "$WORK/bridge-status.json" 200
python3 - "$WORK/bridge-funded-account.json" "$WORK/bridge-status.json" <<'PY'
import json, sys
account = json.load(open(sys.argv[1], encoding="utf-8"))
status = json.load(open(sys.argv[2], encoding="utf-8"))
assert account["balance_nanos"] == 5_000_000_000, account
assert status["deposit_cursor"] == 1, status
PY
pass "real MockUSDC deposit -> confirmed indexer -> exact Sybil credit"

BRIDGE_WITHDRAW_UNITS=2000000
BRIDGE_L1_HEIGHT="$(cast block-number --rpc-url "$ANVIL_RPC")"
BRIDGE_EXPIRY_HEIGHT="$((BRIDGE_L1_HEIGHT + 1000))"
http_json GET /v1/health "$WORK/bridge-health.json" 200
BRIDGE_GENESIS_HASH="$(jget "$WORK/bridge-health.json" genesis_hash)"
[[ "$BRIDGE_GENESIS_HASH" =~ ^[0-9a-f]{64}$ ]]
target/debug/examples/smoke_sign withdrawal \
    --priv "$BRIDGE_PRIVATE_KEY" \
    --account "$BRIDGE_ACCOUNT" \
    --chain-id 11155111 \
    --vault "$BRIDGE_VAULT" \
    --recipient "$ANVIL_ADMIN" \
    --token "$BRIDGE_TOKEN" \
    --amount "$BRIDGE_WITHDRAW_UNITS" \
    --expiry "$BRIDGE_EXPIRY_HEIGHT" \
    --nonce 1 \
    --genesis-hash "$BRIDGE_GENESIS_HASH" >"$WORK/bridge-withdraw-signature.json"
BRIDGE_WITHDRAW_BODY="$(python3 - \
    "$WORK/bridge-withdraw-signature.json" "$BRIDGE_ACCOUNT" "$BRIDGE_VAULT" \
    "$ANVIL_ADMIN" "$BRIDGE_TOKEN" "$BRIDGE_WITHDRAW_UNITS" "$BRIDGE_EXPIRY_HEIGHT" <<'PY'
import json, sys
signature = json.load(open(sys.argv[1], encoding="utf-8"))
print(json.dumps({
    "withdrawal": {
        "account_id": int(sys.argv[2]),
        "chain_id": 11155111,
        "vault_address_hex": sys.argv[3],
        "recipient_hex": sys.argv[4],
        "token_address_hex": sys.argv[5],
        "amount_token_units": int(sys.argv[6]),
        "expiry_height": int(sys.argv[7]),
        "nonce": 1,
    },
    "signer_pubkey_hex": signature["signer_pubkey_hex"],
    "auth_scheme": "raw_p256",
    "signature_hex": signature["signature_hex"],
}))
PY
)"
http_json POST /v1/bridge/withdrawals/signed \
    "$WORK/bridge-withdrawal.json" 200 "$BRIDGE_WITHDRAW_BODY"
BRIDGE_WITHDRAW_HEIGHT="$(jget "$WORK/bridge-withdrawal.json" created_at_height)"
BRIDGE_NULLIFIER="$(jget "$WORK/bridge-withdrawal.json" nullifier_hex)"

http_json POST /v1/simulation/resume "$WORK/bridge-resume.json" 200
bridge_committed=0
for _ in $(seq 1 30); do
    if http_json GET /v1/blocks/latest "$WORK/bridge-latest.json" 200 2>/dev/null \
       && [[ "$(jget "$WORK/bridge-latest.json" height)" -ge "$BRIDGE_WITHDRAW_HEIGHT" ]] \
       && http_json GET "/v1/da/$BRIDGE_WITHDRAW_HEIGHT/manifest" \
            "$WORK/bridge-manifest.json" 200 2>/dev/null; then
        bridge_committed=1
        break
    fi
    sleep 1
done
[[ "$bridge_committed" -eq 1 ]] || { echo "withdrawal block/manifest did not commit" >&2; exit 1; }
http_json POST /v1/simulation/pause "$WORK/bridge-pause.json" 200
http_json GET "/v1/blocks/$BRIDGE_WITHDRAW_HEIGHT" "$WORK/bridge-block.json" 200
http_json GET "/v1/accounts/$BRIDGE_ACCOUNT" "$WORK/bridge-debited-account.json" 200
[[ "$(jget "$WORK/bridge-debited-account.json" balance_nanos)" == "3000000000" ]]

BRIDGE_USER_BEFORE="$(cast call "$BRIDGE_TOKEN" "balanceOf(address)(uint256)" "$ANVIL_ADMIN" --rpc-url "$ANVIL_RPC")"
BRIDGE_VAULT_BEFORE="$(cast call "$BRIDGE_TOKEN" "balanceOf(address)(uint256)" "$BRIDGE_VAULT" --rpc-url "$ANVIL_RPC")"

export SYBIL_API_URL="$BASE"
export SYBIL_SERVICE_TOKEN=sybil-itest-relay
./scripts/relay-sepolia-mock-withdrawals.sh >"$WORK/bridge-relay.log"

# A crash after the vault transaction but before the indexer advances API
# status must not submit another root or request the same nullifier again.
BRIDGE_SETTLEMENT_HEIGHT_BEFORE="$(cast call "$BRIDGE_SETTLEMENT" 'latestHeight()(uint64)' --rpc-url "$ANVIL_RPC")"
./scripts/relay-sepolia-mock-withdrawals.sh >"$WORK/bridge-relay-rerun.log"
BRIDGE_SETTLEMENT_HEIGHT_AFTER="$(cast call "$BRIDGE_SETTLEMENT" 'latestHeight()(uint64)' --rpc-url "$ANVIL_RPC")"
[[ "$BRIDGE_SETTLEMENT_HEIGHT_BEFORE" == "$BRIDGE_SETTLEMENT_HEIGHT_AFTER" ]]
grep -q 'already_queued=1' "$WORK/bridge-relay-rerun.log"

run_bridge_indexer 2>&1 | tee "$WORK/bridge-indexer-queued.log"
http_json GET "/v1/accounts/$BRIDGE_ACCOUNT/withdrawals" \
    "$WORK/bridge-withdrawals-queued.json" 200
python3 - "$WORK/bridge-withdrawals-queued.json" "$BRIDGE_NULLIFIER" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], encoding="utf-8"))
assert len(rows) == 1, rows
assert rows[0]["nullifier_hex"] == sys.argv[2], rows[0]
assert rows[0]["l1_status"] == "queued", rows[0]
assert rows[0]["l1_requested_at_unix"] is not None, rows[0]
assert rows[0]["l1_executable_at_unix"] is not None, rows[0]
PY

cast rpc --rpc-url "$ANVIL_RPC" evm_increaseTime 3601 >/dev/null
cast rpc --rpc-url "$ANVIL_RPC" evm_mine >/dev/null
cast send "$BRIDGE_VAULT" "finalizeWithdrawal(bytes32)" "0x$BRIDGE_NULLIFIER" \
    --rpc-url "$ANVIL_RPC" --private-key "$ANVIL_KEY" >/dev/null
BRIDGE_USER_AFTER="$(cast call "$BRIDGE_TOKEN" "balanceOf(address)(uint256)" "$ANVIL_ADMIN" --rpc-url "$ANVIL_RPC")"
BRIDGE_VAULT_AFTER="$(cast call "$BRIDGE_TOKEN" "balanceOf(address)(uint256)" "$BRIDGE_VAULT" --rpc-url "$ANVIL_RPC")"
BRIDGE_USER_BEFORE="$(cast to-dec "${BRIDGE_USER_BEFORE%% *}")"
BRIDGE_USER_AFTER="$(cast to-dec "${BRIDGE_USER_AFTER%% *}")"
BRIDGE_VAULT_BEFORE="$(cast to-dec "${BRIDGE_VAULT_BEFORE%% *}")"
BRIDGE_VAULT_AFTER="$(cast to-dec "${BRIDGE_VAULT_AFTER%% *}")"
python3 - "$BRIDGE_USER_BEFORE" "$BRIDGE_USER_AFTER" \
    "$BRIDGE_VAULT_BEFORE" "$BRIDGE_VAULT_AFTER" "$BRIDGE_WITHDRAW_UNITS" <<'PY'
import sys
user_before, user_after, vault_before, vault_after, amount = map(int, sys.argv[1:])
assert user_after - user_before == amount, (user_before, user_after, amount)
assert vault_before - vault_after == amount, (vault_before, vault_after, amount)
PY

run_bridge_indexer 2>&1 | tee "$WORK/bridge-indexer-withdrawal.log"
http_json GET "/v1/accounts/$BRIDGE_ACCOUNT/withdrawals" \
    "$WORK/bridge-withdrawals-final.json" 200
python3 - "$WORK/bridge-withdrawals-final.json" "$BRIDGE_NULLIFIER" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], encoding="utf-8"))
assert len(rows) == 1, rows
assert rows[0]["nullifier_hex"] == sys.argv[2], rows[0]
assert rows[0]["l1_status"] == "finalized", rows[0]
assert rows[0]["l1_requested_at_unix"] is not None, rows[0]
assert rows[0]["l1_finalized_at_unix"] is not None, rows[0]
PY
pass "signed debit -> validated mock relay -> idempotent rerun -> delayed queue/finalize -> indexed status"

step "Compose integration passed"
