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

for compose_file in docker-compose.yml docker-compose.prod.yml; do
    grep -Fq -- '"--metrics-port=9101"' "$compose_file" \
        || fail "$compose_file does not enable the arena metrics exporter"
done
grep -Fq 'targets: ["sybil-arena:9101"]' deploy/prometheus.yml \
    || fail "VictoriaMetrics does not scrape the arena metrics exporter"
pass "arena desired-state metrics are enabled and scraped in dev/prod compose"

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

durability_contract=$(
    compose config | python3 -c '
import re
import sys

keys = (
    "SYBIL_DATA_DIR",
    "SYBIL_MARKET_REF_DATA_PATH",
    "SYBIL_ADMIN_FEED_KEY_PATH",
)
environment = {}
service_lines = []
in_api = False
for line in sys.stdin:
    if line.rstrip() == "  sybil-api:":
        in_api = True
        continue
    if in_api and re.match(r"^  [^ ]", line):
        break
    if in_api:
        service_lines.append(line)
        match = re.match(r"^      ([A-Z0-9_]+): (.*)$", line.rstrip())
        if match:
            environment[match.group(1)] = match.group(2).strip("\"\047")
for key in keys:
    print(f"{key}={environment.get(key)}")
service = "".join(service_lines)
has_data_volume = (
    "source: sybil-data" in service and "target: /data" in service
) or re.search(r"^\s*-\s+sybil-data:/data(?::|$)", service, re.MULTILINE)
data_volume = (
    "sybil-data"
    if has_data_volume
    else None
)
print(f"SYBIL_DATA_VOLUME={data_volume}")
'
)
expected_durability_contract=$(printf '%s\n' \
    'SYBIL_DATA_DIR=/data' \
    'SYBIL_MARKET_REF_DATA_PATH=/data/market_ref_data.json' \
    'SYBIL_ADMIN_FEED_KEY_PATH=/data/admin-feed.key' \
    'SYBIL_DATA_VOLUME=sybil-data')
[[ "$durability_contract" == "$expected_durability_contract" ]] \
    || fail "production compose does not persist API state, mirror metadata, and the admin feed key in sybil-data"
pass "production compose persists the admin feed key and API data in sybil-data"

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

# The Polymarket mirror bind-mounts its checked-in catalogs from the source
# tree on the host. Keep deploy-sync responsible for creating that remote path
# and copying both files so a fresh box cannot start with stale/missing inputs.
deploy_sync_recipe=$(
    awk '
        /^deploy-sync:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq '/opt/sybil/crates/sybil-polymarket' <<<"$deploy_sync_recipe" \
    || fail "deploy-sync does not create the remote Polymarket catalog directory"
for catalog in curated_markets.json native_markets.json; do
    grep -Fq "crates/sybil-polymarket/$catalog" <<<"$deploy_sync_recipe" \
        || fail "deploy-sync does not transfer $catalog"
done
pass "deploy-sync transfers both bind-mounted Polymarket catalogs"

deploy_verify_recipe=$(
    awk '
        /^deploy-verify:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer' <<<"$deploy_verify_recipe" \
    || fail "deploy-verify does not require the signed order/cancel smoke helper"
if grep -Fq -- '--skip-fill-seed' <<<"$deploy_verify_recipe"; then
    fail "deploy-verify must retain the full deterministic fill seed"
fi
grep -Fq -- '--service-token' <<<"$deploy_verify_recipe" \
    || fail "deploy-verify lost the valid service-token gating checks"
pass "deploy-verify fails closed when signed order/cancel smoke cannot run"

deploy_verify_scoped_recipe=$(
    awk '
        /^deploy-verify-scoped:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer --skip-fill-seed' \
    <<<"$deploy_verify_scoped_recipe" \
    || fail "deploy-verify-scoped does not skip only the persistent fill fixture"
grep -Fq -- '--service-token' <<<"$deploy_verify_scoped_recipe" \
    || fail "deploy-verify-scoped lost the valid service-token gating checks"
grep -Eq '^deploy-web:.*&& deploy-verify-scoped$' justfile \
    || fail "deploy-web does not use the scoped post-deploy verifier"
grep -Eq '^deploy-arena:.*&& deploy-verify-scoped$' justfile \
    || fail "deploy-arena does not use the scoped post-deploy verifier"
grep -Eq '^deploy-api:.*&& deploy-verify$' justfile \
    || fail "deploy-api no longer uses the full deterministic-fill verifier"
grep -Eq '^deploy-all:.*&& deploy-verify$' justfile \
    || fail "deploy-all no longer uses the full deterministic-fill verifier"
pass "scoped web/arena deploys avoid persistent fill fixtures; API/all remain full"

grep -Eq '^COPY[[:space:]]+scripts/[[:space:]]+scripts/$' arena/Dockerfile \
    || fail "arena image does not include offline calibration scripts"
for recipe in arena-outcomes-dry-run arena-record-outcomes arena-calibration; do
    grep -Eq "^${recipe}:" justfile \
        || fail "justfile is missing the ${recipe} operator recipe"
done
pass "arena image and operator recipes expose live calibration tooling"
