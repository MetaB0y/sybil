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

COMPOSE_CMD=${COMPOSE_CMD:-docker compose}
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

if contains_service "$default_services" sybil-prover-worker; then
    fail "default compose config unexpectedly includes sybil-prover-worker"
fi
pass "default compose config excludes sybil-prover-worker"

profile_services=$(COMPOSE_PROFILES=prover-worker compose config --services)
contains_service "$profile_services" sybil-prover-worker \
    || fail "prover-worker profile does not include sybil-prover-worker"
pass "prover-worker profile includes sybil-prover-worker"

compose config --quiet
COMPOSE_PROFILES=prover-worker compose config --quiet
pass "compose config parses with and without prover-worker profile"

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
