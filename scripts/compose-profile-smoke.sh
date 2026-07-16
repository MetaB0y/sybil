#!/usr/bin/env bash
set -euo pipefail

# Smoke-test Docker Compose profile boundaries without starting containers.

cd "$(dirname "$0")/.."

export OPENROUTER_API_KEY="${OPENROUTER_API_KEY:-}"
export SYBIL_SERVICE_TOKEN="${SYBIL_SERVICE_TOKEN:-compose-smoke-service-token}"
export SYBIL_HISTORY_TOKEN="${SYBIL_HISTORY_TOKEN:-compose-smoke-history-token}"
export SYBIL_ARENA_READ_TOKEN="${SYBIL_ARENA_READ_TOKEN:-compose-smoke-arena-token}"
export SYBIL_WEBAUTHN_RP_ID="${SYBIL_WEBAUTHN_RP_ID:-app.example.test}"
export SYBIL_WEBAUTHN_ORIGIN="${SYBIL_WEBAUTHN_ORIGIN:-https://app.example.test}"
export GF_SECURITY_ADMIN_PASSWORD="${GF_SECURITY_ADMIN_PASSWORD:-compose-smoke-grafana-password}"
export CADDY_OPS_AUTH_USER="${CADDY_OPS_AUTH_USER:-ops}"
export CADDY_OPS_AUTH_HASH="${CADDY_OPS_AUTH_HASH:-compose-smoke-caddy-hash}"
export SYBIL_L1_RPC_URLS="${SYBIL_L1_RPC_URLS:-http://rpc-a.example.test,http://rpc-b.example.test}"
export SYBIL_L1_RPC_IDS="${SYBIL_L1_RPC_IDS:-compose-smoke-a,compose-smoke-b}"

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

profile_contract=$(
    compose config | python3 -c '
import sys
import yaml

config = yaml.safe_load(sys.stdin)
for service, body in sorted(config["services"].items()):
    profiles = body.get("profiles", [])
    profile = ",".join(profiles) if profiles else "core"
    print(f"{service}={profile}")
'
)
expected_profile_contract=$(printf '%s\n' \
    'caddy=core' \
    'grafana=ops' \
    'node-exporter=ops' \
    'sybil-api=core' \
    'sybil-arena=integrations' \
    'sybil-arena-dashboard=integrations' \
    'sybil-history=core' \
    'sybil-l1-indexer=l1-indexer' \
    'sybil-native-admin=integrations' \
    'sybil-native-mm=integrations' \
    'sybil-polymarket=integrations' \
    'sybil-prover=validity' \
    'sybil-web=core' \
    'victoriametrics=ops' \
    'vmalert=ops')
[[ "$profile_contract" == "$expected_profile_contract" ]] \
    || fail "Compose services drifted across core/integration/validity/ops boundaries"
pass "default core and optional subsystem memberships are explicit"

if ! SYBIL_L1_RPC_URLS= SYBIL_L1_RPC_IDS= compose config --services >/dev/null; then
    fail "inactive L1 indexer credentials block the default private-devnet stack"
fi
pass "inactive L1 credentials do not block private-devnet deploys"

available_profiles=$(compose config --profiles)
for profile in integrations validity ops l1-indexer; do
    contains_service "$available_profiles" "$profile" \
        || fail "Compose profile $profile is not declared"
done

for profile in integrations validity ops; do
    compose --profile "$profile" config --quiet \
        || fail "$profile profile does not compose cleanly"
done
compose --profile integrations --profile validity --profile ops config --quiet \
    || fail "full ordinary profile set does not compose cleanly"
pass "integration, validity, and ops profiles are isolated and compose cleanly"

for variable in COMPOSE_PROD COMPOSE_TELEGRAM; do
    definition=$(grep -E "^${variable} :=" justfile)
    for profile in integrations ops; do
        grep -Fq -- "--profile $profile" <<<"$definition" \
            || fail "$variable does not explicitly select the $profile profile"
    done
    if grep -Fq -- '--profile validity' <<<"$definition"; then
        fail "$variable silently enables validity on the product devnet"
    fi
done
validity_definition=$(grep -E '^COMPOSE_PROD_VALIDITY :=' justfile)
for profile in integrations ops validity; do
    grep -Fq -- "--profile $profile" <<<"$validity_definition" \
        || fail "COMPOSE_PROD_VALIDITY does not explicitly select the $profile profile"
done
pass "product and explicit-validity production topologies are separate"

compose --profile l1-indexer config --quiet \
    || fail "l1-indexer profile does not compose cleanly"
pass "L1 indexer is explicit opt-in deployment state"

compose config --quiet
pass "compose config parses"

prover_service_block=$(
    awk '
        /^  sybil-prover:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
grep -Fq '"daemon"' <<<"$prover_service_block" \
    || fail "sybil-prover does not run the durable daemon"
grep -Fq 'prover-data:/data' <<<"$prover_service_block" \
    || fail "sybil-prover does not mount its durable redb volume"
grep -Fq 'SYBIL_PROVER_SOURCE_URL' <<<"$prover_service_block" \
    || fail "sybil-prover is not wired to the authenticated source outbox"
grep -Fq 'SYBIL_PROVER_PROOF_KIND' <<<"$prover_service_block" \
    || fail "sybil-prover has no explicit typed backend"
grep -Fq 'mem_limit: "384m"' <<<"$prover_service_block" \
    || fail "sybil-prover lost its integration safety ceiling"
grep -Fq 'memswap_limit: "512m"' <<<"$prover_service_block" \
    || fail "sybil-prover has no bounded transient-memory cushion"
grep -Fq 'restart: "on-failure:3"' <<<"$prover_service_block" \
    || fail "sybil-prover can enter an unbounded OOM restart loop"
pass "explicit prover integration has durable state and bounded failure behavior"

# The restore drill must be usable without the base file. Merging Compose
# volume lists would append the globally named sybil-data mount and make
# throwaway `down -v` cleanup capable of deleting stopped application state.
# shellcheck disable=SC2086
restore_drill_config=$(COMPOSE_PROJECT_NAME=sybil-restore-contract \
    $COMPOSE_CMD -f docker-compose.itest.yml config)
restore_drill_api_block=$(
    awk '
        /^  sybil-api:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' <<<"$restore_drill_config"
)
grep -Fq 'itest-data:/itest-data' <<<"$restore_drill_api_block" \
    || fail "standalone restore drill does not mount its unique itest-data volume"
if grep -Fq 'sybil-data:/data' <<<"$restore_drill_api_block"; then
    fail "standalone restore drill references the global sybil-data volume"
fi
grep -Fq -- '--allow-live-host' scripts/store-restore-drill.sh \
    || fail "restore drill has no explicit live-host resource override"
grep -Fq "trap 'cleanup 129' HUP" scripts/store-restore-drill.sh \
    || fail "restore drill does not clean up after an SSH hangup"
pass "restore drills isolate volumes, reject implicit live-host sharing, and trap hangups"

l1_indexer_service_block=$(
    awk '
        /^  sybil-l1-indexer:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
for expected in \
    'entrypoint: ["sybil-l1-indexer"]' \
    'SYBIL_L1_TRUST_MODE: "${SYBIL_L1_TRUST_MODE:-unsafe-single-dev}"' \
    'SYBIL_L1_RPC_IDS: "${SYBIL_L1_RPC_IDS:-local-anvil}"' \
    'SYBIL_L1_CURSOR_PATH: "/l1-indexer-data/cursor.json"' \
    'SYBIL_L1_METRICS_BIND: "0.0.0.0:9102"' \
    'l1-indexer-data:/l1-indexer-data' \
    'http://127.0.0.1:9102/healthz'; do
    grep -Fq "$expected" <<<"$l1_indexer_service_block" \
        || fail "L1 indexer deployment is missing $expected"
done
grep -Fq '/app/bin/sybil-l1-indexer' Dockerfile \
    || fail "server image does not package sybil-l1-indexer"
if grep -Eq 'job_name: sybil-(prover|l1-indexer)' deploy/prometheus.yml; then
    fail "product monitoring statically couples to an absent optional profile"
fi
pass "L1 indexer binary and durable health boundary are wired without fake product targets"

polymarket_service_block=$(
    awk '
        /^  sybil-polymarket:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
for expected in \
    '"--monitoring-bind=0.0.0.0:9105"' \
    'http://127.0.0.1:9105/readyz'; do
    grep -Fq -- "$expected" <<<"$polymarket_service_block" \
        || fail "Polymarket integration monitoring is missing $expected"
done
grep -Fq 'targets: ["sybil-polymarket:9105"]' deploy/prometheus.yml \
    || fail "VictoriaMetrics does not scrape the Polymarket owner process"
pass "Polymarket owner process exposes readiness and scraped actor progress"

native_mm_service_block=$(
    awk '
        /^  sybil-native-mm:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
for expected in \
    '"--monitoring-bind=0.0.0.0:9104"' \
    'http://127.0.0.1:9104/readyz'; do
    grep -Fq -- "$expected" <<<"$native_mm_service_block" \
        || fail "native MM monitoring is missing $expected"
done
grep -Fq 'targets: ["sybil-native-mm:9104"]' deploy/prometheus.yml \
    || fail "VictoriaMetrics does not scrape the native MM owner process"
pass "native MM owner process exposes readiness and scraped quote progress"

for compose_file in docker-compose.yml docker-compose.prod.yml; do
    grep -Fq -- '"--metrics-port=9101"' "$compose_file" \
        || fail "$compose_file does not enable the arena metrics exporter"
done
grep -Fq 'ARENA_READ_API_PORT: "9103"' docker-compose.yml \
    || fail "base Compose does not enable the private Arena read API"
grep -Fq 'SYBIL_ARENA_READ_URL: "http://sybil-arena:9103"' docker-compose.yml \
    || fail "sybil-api is not wired to the typed Arena read boundary"
api_service_block=$(
    awk '
        /^  sybil-api:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
if grep -Fq 'arena-data:' <<<"$api_service_block"; then
    fail "sybil-api still mounts Python-owned Arena storage"
fi
grep -Fq 'targets: ["sybil-arena:9101"]' deploy/prometheus.yml \
    || fail "VictoriaMetrics does not scrape the arena metrics exporter"
pass "Arena owns its authenticated read boundary, storage, and scraped metrics"

prod_arena_service_block=$(
    awk '
        /^  sybil-arena:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.prod.yml
)
for argument in \
    '"--market-profile=important-news"' \
    '"--max-markets=12"' \
    '"--require-reference-prices"'; do
    grep -Fq -- "$argument" <<<"$prod_arena_service_block" \
        || fail "production arena does not pin focused reference-backed market selection ($argument)"
done
pass "production arena selects a bounded reference-backed news cohort"

arena_service_block=$(
    awk '
        /^  sybil-arena:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
grep -Fq 'http://127.0.0.1:9101/metrics' <<<"$arena_service_block" \
    || fail "sybil-arena has no metrics-readiness healthcheck"

arena_dashboard_block=$(
    awk '
        /^  sybil-arena-dashboard:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
grep -Fq 'http://127.0.0.1:8501/_stcore/health' <<<"$arena_dashboard_block" \
    || fail "sybil-arena-dashboard has no Streamlit readiness healthcheck"
grep -Fq 'sybil-arena:' <<<"$arena_dashboard_block" \
    || fail "sybil-arena-dashboard does not depend on sybil-arena"
grep -Fq 'condition: service_healthy' <<<"$arena_dashboard_block" \
    || fail "sybil-arena-dashboard does not wait for healthy arena metrics"
pass "arena runner and dashboard expose healthchecks with ordered startup"

base_api_service_block=$(
    awk '
        /^  sybil-api:/ { in_service = 1; next }
        in_service && /^  [[:alnum:]_-]+:/ { exit }
        in_service { print }
    ' docker-compose.yml
)
for expected in \
    'SYBIL_DEPLOYMENT_PROFILE: "${SYBIL_DEPLOYMENT_PROFILE:-devnet}"' \
    'SYBIL_PUBLIC_ACCOUNT_CAPACITY: "${SYBIL_PUBLIC_ACCOUNT_CAPACITY:-1000}"' \
    'SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS: "${SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS:-1000000000000}"' \
    'SYBIL_HTTP_ONBOARDING_GLOBAL_RPS: "5"' \
    'SYBIL_HTTP_ONBOARDING_CLIENT_RPS: "1"' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS: "${SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS:-8640}"' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS: "60"' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_MAX_ROWS_PER_PASS: "1000"'; do
    grep -Fq "$expected" <<<"$base_api_service_block" \
        || fail "devnet compose is missing bounded proof-job policy $expected"
done
pass "devnet compose identifies its profile and bounds onboarding plus proof-job stock"

retention_env=$(
    compose config | python3 -c '
import re
import sys

keys = (
    "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS",
    "SYBIL_BLOCK_INTERVAL_MS",
    "SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS",
    "SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS",
    "SYBIL_ACKNOWLEDGED_PROOF_JOB_MAX_ROWS_PER_PASS",
    "SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS",
    "SYBIL_CANONICAL_ARCHIVE_MAINTENANCE_INTERVAL_BLOCKS",
    "SYBIL_CANONICAL_ARCHIVE_MAX_ROWS_PER_PASS",
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
    'SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS=0' \
    'SYBIL_BLOCK_INTERVAL_MS=10000' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS=60480' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS=60' \
    'SYBIL_ACKNOWLEDGED_PROOF_JOB_MAX_ROWS_PER_PASS=10000' \
    'SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS=60480' \
    'SYBIL_CANONICAL_ARCHIVE_MAINTENANCE_INTERVAL_BLOCKS=60' \
    'SYBIL_CANONICAL_ARCHIVE_MAX_ROWS_PER_PASS=10000')
[[ "$retention_env" == "$expected_retention_env" ]] \
    || fail "production compose does not pin archive and acknowledged proof-job retention"
pass "production compose pins market, archive, and acknowledged proof-job retention"

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

reset_recipe=$(
    awk '
        /^deploy-reset-state / { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
for volume in sybil-data history-data polymarket-data native-data arena-data prover-data \
    prover-artifacts l1-indexer-data vmdata; do
    grep -Fq "$volume" <<<"$reset_recipe" \
        || fail "fresh-genesis reset does not clear $volume"
done
grep -Fq -- '--profile l1-indexer down' <<<"$reset_recipe" \
    || fail "fresh-genesis reset does not stop the optional L1 sidecar"
pass "fresh-genesis reset clears every durable application subsystem"

# Polymarket curation and native provisioning have separate owners and remote
# paths. Keep deploy-sync responsible for both checked-in inputs.
deploy_sync_recipe=$(
    awk '
        /^deploy-sync:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq '/opt/sybil/crates/sybil-polymarket' <<<"$deploy_sync_recipe" \
    || fail "deploy-sync does not create the remote Polymarket catalog directory"
grep -Fq 'crates/sybil-polymarket/curated_markets.json' <<<"$deploy_sync_recipe" \
    || fail "deploy-sync does not transfer Polymarket curation"
grep -Fq '/opt/sybil/crates/sybil-native' <<<"$deploy_sync_recipe" \
    || fail "deploy-sync does not create the remote native catalog directory"
grep -Fq 'crates/sybil-native/native_markets.json' <<<"$deploy_sync_recipe" \
    || fail "deploy-sync does not transfer native provisioning input"
pass "deploy-sync preserves separate Polymarket and native catalog ownership"

deploy_probe_recipe=$(
    awk '
        /^deploy-install-synthetic-probe:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
for expected in \
    'sybil-synthetic-probe.service' \
    'sybil-synthetic-probe.timer' \
    'systemctl daemon-reload' \
    'systemctl enable --now sybil-synthetic-probe.timer'; do
    grep -Fq "$expected" <<<"$deploy_probe_recipe" \
        || fail "scheduled probe deployment is missing $expected"
done
grep -Eq '^deploy-monitoring:.*deploy-install-synthetic-probe' justfile \
    || fail "monitoring deploy does not converge the scheduled probe"
grep -Eq '^deploy-all:.*deploy-install-synthetic-probe' justfile \
    || fail "all-stack deploy does not converge the scheduled probe"
pass "monitoring deploys converge checked-in scripts and systemd units"

deploy_verify_recipe=$(
    awk '
        /^deploy-verify:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer' <<<"$deploy_verify_recipe" \
    || fail "deploy-verify does not require the signed order/cancel smoke helper"
if grep -Fq -- '--require-proof-freshness' <<<"$deploy_verify_recipe"; then
    fail "product deploy verification silently requires the absent validity profile"
fi
if grep -Fq -- '--skip-fill-seed' <<<"$deploy_verify_recipe"; then
    fail "deploy-verify must retain the full deterministic fill seed"
fi
grep -Fq -- '--service-token' <<<"$deploy_verify_recipe" \
    || fail "deploy-verify lost the valid service-token gating checks"
pass "deploy-verify fails closed when signed order/cancel smoke cannot run"

deploy_verify_validity_recipe=$(
    awk '
        /^deploy-verify-validity:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer --require-proof-freshness --skip-fill-seed' \
    <<<"$deploy_verify_validity_recipe" \
    || fail "explicit validity promotion lost proof freshness or its scoped fill skip"
grep -Fq 'Environment=SYBIL_SMOKE_PROOF_LAG=off' deploy/systemd/sybil-synthetic-probe.service \
    || fail "product synthetic timer does not explicitly disable absent validity"
pass "proof freshness is required only by the explicit validity topology"

grep -Fq 'check_public_block_stream' scripts/post-deploy-smoke.sh \
    || fail "post-deploy smoke no longer runs the public block-stream check"
grep -Fq '/v2/blocks/ws?from_block=' scripts/post-deploy-smoke.sh \
    || fail "post-deploy smoke does not target the public v2 replay endpoint"
grep -Fq '_ws_resume_check.py' scripts/post-deploy-smoke.sh \
    || fail "post-deploy smoke lost the dependency-free WebSocket checker"
grep -Fq 'deployed web bundle targets the public /v2 block stream' scripts/post-deploy-smoke.sh \
    || fail "post-deploy smoke no longer checks the web bundle's realtime protocol"
pass "post-deploy smoke requires matching web/API v2 realtime plus replay/live handoff"

deploy_verify_scoped_recipe=$(
    awk '
        /^deploy-verify-scoped:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer --skip-fill-seed' \
    <<<"$deploy_verify_scoped_recipe" \
    || fail "deploy-verify-scoped lost its scoped fill skip"
if grep -Fq -- '--require-proof-freshness' <<<"$deploy_verify_scoped_recipe"; then
    fail "Arena deploy verification silently requires the absent validity profile"
fi
if grep -Fq -- '--skip-mirror-readiness' <<<"$deploy_verify_scoped_recipe"; then
    fail "Arena deploy verification must require external mirror readiness"
fi
grep -Fq -- '--service-token' <<<"$deploy_verify_scoped_recipe" \
    || fail "deploy-verify-scoped lost the valid service-token gating checks"

deploy_verify_web_recipe=$(
    awk '
        /^deploy-verify-web:/ { in_recipe = 1; next }
        in_recipe && /^[[:alnum:]_-]+[^:]*:/ { exit }
        in_recipe { print }
    ' justfile
)
grep -Fq 'post-deploy-smoke.sh --require-signer --skip-fill-seed --skip-mirror-readiness' \
    <<<"$deploy_verify_web_recipe" \
    || fail "deploy-verify-web lost its two isolated skips"
if grep -Fq -- '--require-proof-freshness' <<<"$deploy_verify_web_recipe"; then
    fail "web deploy verification silently requires the absent validity profile"
fi
grep -Fq -- '--service-token' <<<"$deploy_verify_web_recipe" \
    || fail "deploy-verify-web lost the valid service-token gating checks"
grep -Eq '^deploy-web:.*&& deploy-verify-web$' justfile \
    || fail "deploy-web does not use the web-only post-deploy verifier"
grep -Eq '^deploy-arena:.*&& deploy-verify-scoped$' justfile \
    || fail "deploy-arena does not use the scoped post-deploy verifier"
grep -Eq '^deploy-api:.*&& deploy-verify$' justfile \
    || fail "deploy-api no longer uses the full deterministic-fill verifier"
grep -Eq '^deploy-all:.*&& deploy-verify$' justfile \
    || fail "deploy-all no longer uses the full deterministic-fill verifier"
pass "web/Arena deploys avoid persistent fill fixtures; only web skips external mirror readiness"

grep -Eq '^COPY[[:space:]]+scripts/[[:space:]]+scripts/$' arena/Dockerfile \
    || fail "arena image does not include offline calibration scripts"
for recipe in arena-outcomes-dry-run arena-record-outcomes arena-calibration; do
    grep -Eq "^${recipe}:" justfile \
        || fail "justfile is missing the ${recipe} operator recipe"
done
pass "arena image and operator recipes expose live calibration tooling"
