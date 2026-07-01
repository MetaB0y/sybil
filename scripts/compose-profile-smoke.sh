#!/usr/bin/env bash
set -euo pipefail

# Smoke-test Docker Compose profile boundaries without starting containers.

cd "$(dirname "$0")/.."

export OPENROUTER_API_KEY="${OPENROUTER_API_KEY:-}"

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
