#!/usr/bin/env bash
set -euo pipefail

# Smoke-test Docker Compose profile boundaries without starting containers.

cd "$(dirname "$0")/.."

export OPENROUTER_API_KEY="${OPENROUTER_API_KEY:-}"
export SYBIL_SERVICE_TOKEN="${SYBIL_SERVICE_TOKEN:-compose-smoke-service-token}"
export SYBIL_WEBAUTHN_RP_ID="${SYBIL_WEBAUTHN_RP_ID:-app.example.test}"
export SYBIL_WEBAUTHN_ORIGIN="${SYBIL_WEBAUTHN_ORIGIN:-https://app.example.test}"
export GF_SECURITY_ADMIN_PASSWORD="${GF_SECURITY_ADMIN_PASSWORD:-compose-smoke-grafana-password}"
export CADDY_OPS_AUTH_USER="${CADDY_OPS_AUTH_USER:-ops}"
export CADDY_OPS_AUTH_HASH="${CADDY_OPS_AUTH_HASH:-compose-smoke-caddy-hash}"

cleanup_arena_env_file=false
if [[ ! -f arena.env ]]; then
    cleanup_arena_env_file=true
    : > arena.env
fi
cleanup() {
    if [[ "$cleanup_arena_env_file" == true ]]; then
        rm -f arena.env
    fi
}
trap cleanup EXIT

if [[ -n "${COMPOSE_CMD:-}" ]]; then
    : # Explicit operator/CI override.
elif docker compose version >/dev/null 2>&1; then
    COMPOSE_CMD="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_CMD="docker-compose"
else
    echo "error: neither 'docker compose' nor 'docker-compose' is available" >&2
    exit 2
fi
COMPOSE_FILES=(-f docker-compose.yml -f docker-compose.prod.yml)

pass() { printf "  \033[32m✓\033[0m %s\n" "$1"; }
fail() { printf "  \033[31m✗\033[0m %s\n" "$1" >&2; exit 1; }

compose() {
    # Intentionally allow COMPOSE_CMD to contain a command plus arguments.
    # shellcheck disable=SC2086
    $COMPOSE_CMD "${COMPOSE_FILES[@]}" "$@"
}

contains_service() {
    local services=$1
    local service=$2
    grep -Fxq "$service" <<<"$services"
}

default_services=$(compose config --services)

for service in sybil-api sybil-polymarket sybil-prover sybil-prover-mock; do
    contains_service "$default_services" "$service" \
        || fail "default compose config is missing $service"
done
pass "default compose config includes core devnet services"

# Compose v2 filters `config --services` by active profiles; legacy
# docker-compose 1.29 lists every declared service even when its profile is
# inactive. Prefer the stronger runtime-config assertion when available, and
# fall back to checking the parsed source boundary on v1.
if contains_service "$default_services" sybil-prover-worker; then
    worker_block=$(
        awk '
            /^  sybil-prover-worker:/ { in_service = 1; next }
            in_service && /^  [[:alnum:]_-]+:/ { exit }
            in_service { print }
        ' docker-compose.yml
    )
    grep -Eq 'profiles:[[:space:]]*\["prover-worker"\]' <<<"$worker_block" \
        || fail "sybil-prover-worker is not gated by the prover-worker profile"
    pass "prover-worker is profile-gated (legacy Compose static check)"
else
    pass "default compose config excludes sybil-prover-worker"
    profile_services=$(COMPOSE_PROFILES=prover-worker compose config --services)
    contains_service "$profile_services" sybil-prover-worker \
        || fail "prover-worker profile does not include sybil-prover-worker"
    pass "prover-worker profile includes sybil-prover-worker"
fi

compose config --quiet
COMPOSE_PROFILES=prover-worker compose config --quiet
pass "compose config parses with and without prover-worker profile"

retention_env=$(
    compose config | python3 -c '
import re
import sys

keys = (
    "SYBIL_BLOCK_INTERVAL_MS",
    "SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS",
    "SYBIL_RAW_PRICE_RETENTION_BLOCKS",
    "SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS",
    "SYBIL_HISTORY_PRUNE_MAX_ROWS",
    "SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS",
    "SYBIL_PRICE_CANDLE_RETENTION_SECS",
)
environment = {}
in_api = False
for line in sys.stdin:
    if line.rstrip() == "  sybil-api:":
        in_api = True
        continue
    if in_api and re.match(r"^  [^ ]", line):
        break
    if in_api:
        match = re.match(r"^      ([A-Z0-9_]+): (.*)$", line.rstrip())
        if match:
            environment[match.group(1)] = match.group(2).strip("\"\047")
for key in keys:
    print(f"{key}={environment.get(key)}")
'
)
expected_retention_env=$(printf '%s\n' \
    'SYBIL_BLOCK_INTERVAL_MS=10000' \
    'SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS=60480' \
    'SYBIL_RAW_PRICE_RETENTION_BLOCKS=60480' \
    'SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS=60' \
    'SYBIL_HISTORY_PRUNE_MAX_ROWS=10000' \
    'SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS=60,300,3600' \
    'SYBIL_PRICE_CANDLE_RETENTION_SECS=604800,604800,604800')
[[ "$retention_env" == "$expected_retention_env" ]] \
    || fail "production compose does not pin the seven-day history retention policy"
pass "production compose pins seven-day block/DA, raw-price, and candle retention"

local_webauthn=$(
    unset SYBIL_WEBAUTHN_RP_ID SYBIL_WEBAUTHN_ORIGIN
    # Intentionally allow COMPOSE_CMD to contain a command plus arguments.
    # shellcheck disable=SC2086
    $COMPOSE_CMD -f docker-compose.yml -f docker-compose.override.yml config \
        | python3 -c '
import re
import sys

environment = {}
in_api = False
for line in sys.stdin:
    if line.rstrip() == "  sybil-api:":
        in_api = True
        continue
    if in_api and re.match(r"^  [^ ]", line):
        break
    if in_api:
        match = re.match(r"^      (SYBIL_WEBAUTHN_(?:RP_ID|ORIGIN)): (.*)$", line.rstrip())
        if match:
            environment[match.group(1)] = match.group(2).strip("\"\047")
for key in ("SYBIL_WEBAUTHN_RP_ID", "SYBIL_WEBAUTHN_ORIGIN"):
    print(f"{key}={environment.get(key)}")
'
)
expected_local_webauthn=$(printf '%s\n' \
    'SYBIL_WEBAUTHN_RP_ID=localhost' \
    'SYBIL_WEBAUTHN_ORIGIN=http://localhost:3005')
[[ "$local_webauthn" == "$expected_local_webauthn" ]] \
    || fail "local Compose WebAuthn RP/origin do not match the published web app"
pass "local Compose passkeys use the published localhost:3005 web origin"

# `deploy-all` builds every application image locally. Keep its save/load stream
# in lockstep so the host cannot silently restart an older image after a build.
deploy_all_save=$(
    awk '
        /^deploy-all:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe && /docker save / { print; exit }
    ' justfile
)
[[ -n "$deploy_all_save" ]] || fail "deploy-all has no docker save command"

for image in sybil-api:latest sybil-arena:latest sybil-web:latest; do
    grep -Eq "(^|[[:space:]])${image}([[:space:]]|$)" <<<"$deploy_all_save" \
        || fail "deploy-all does not transfer $image"
done
grep -Fq '| ssh {{SERVER}} docker load' <<<"$deploy_all_save" \
    || fail "deploy-all does not stream its images to the remote Docker daemon"
pass "deploy-all transfers every locally built application image"

deploy_verify_recipe=$(
    awk '
        /^deploy-verify:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer' <<<"$deploy_verify_recipe" \
    || fail "deploy-verify does not require the signed order/cancel smoke helper"
pass "deploy-verify fails closed when signed order/cancel smoke cannot run"

grep -Eq '^COPY[[:space:]]+scripts/[[:space:]]+scripts/$' arena/Dockerfile \
    || fail "arena image does not include offline calibration scripts"
for recipe in arena-outcomes-dry-run arena-record-outcomes arena-calibration; do
    grep -Eq "^${recipe}:" justfile \
        || fail "justfile is missing the ${recipe} operator recipe"
done
pass "arena image and operator recipes expose live calibration tooling"
