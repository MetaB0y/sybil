#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

readonly PROJECT="sybil-local-soak"
readonly STATE_DIR=".sybil-soak"
readonly ENV_FILE="$STATE_DIR/soak.env"
readonly CREDENTIALS_FILE="$STATE_DIR/actor-credentials.json"
readonly COMPOSE_FILES=(
    -f docker-compose.yml
    -f docker-compose.override.yml
    -f docker-compose.soak.yml
)
readonly SERVICES=(
    sybil-api
    sybil-history
    sybil-soak-bootstrap
    sybil-polymarket
    sybil-arena
    sybil-arena-dashboard
    sybil-web
    sybil-prover
    victoriametrics
    vmalert
    grafana
)
readonly BUILD_SERVICES=(sybil-api sybil-arena sybil-web)

die() {
    echo "local soak: $*" >&2
    exit 2
}

require_local_docker() {
    command -v docker >/dev/null 2>&1 || die "Docker Desktop is required"
    docker compose version >/dev/null 2>&1 || die "Docker Compose v2 is required"
    if [[ -n "${DOCKER_HOST:-}" ]]; then
        case "$DOCKER_HOST" in
            unix://*|npipe://*) ;;
            *) die "refusing DOCKER_HOST=$DOCKER_HOST; use the local Docker Desktop engine" ;;
        esac
    fi
    local endpoint
    endpoint=$(docker context inspect "$(docker context show)" --format '{{(index .Endpoints "docker").Host}}')
    case "$endpoint" in
        unix://*|npipe://*) ;;
        *) die "refusing non-local Docker endpoint: $endpoint" ;;
    esac
    docker info >/dev/null 2>&1 || die "Docker Desktop is installed but its engine is not running"
}

random_token() {
    openssl rand -hex 24
}

validate_existing_credentials() {
    python3 - "$CREDENTIALS_FILE" "$ENV_FILE" <<'PY'
import json, pathlib, sys

actors = json.loads(pathlib.Path(sys.argv[1]).read_text()).get("actors", [])
mm = [row for row in actors if row.get("role") == "market_maker"]
noise = [row for row in actors if row.get("role") == "noise"]
env = dict(
    line.split("=", 1)
    for line in pathlib.Path(sys.argv[2]).read_text().splitlines()
    if line and not line.startswith("#") and "=" in line
)
valid = (
    len(actors) == 16
    and len(mm) == 1
    and len(noise) == 15
    and len({row.get("principal_id") for row in actors}) == 16
    and len({row.get("account_id") for row in actors}) == 16
    and len({row.get("token") for row in actors}) == 16
    and env.get("SYBIL_SOAK_MM_TOKEN") == mm[0].get("token")
)
raise SystemExit(0 if valid else 1)
PY
}

prepare() {
    require_local_docker
    command -v openssl >/dev/null 2>&1 || die "openssl is required to generate local actor tokens"
    mkdir -p "$STATE_DIR"
    chmod 700 "$STATE_DIR"
    if [[ -f "$ENV_FILE" || -f "$CREDENTIALS_FILE" ]]; then
        if [[ -f "$ENV_FILE" && -f "$CREDENTIALS_FILE" ]] \
            && validate_existing_credentials; then
            return
        fi
        die "legacy or partial local-soak credentials found; preserve them externally if needed, then run 'just local-soak-clean' before starting v2.1"
    fi

    local mm_token
    mm_token=$(random_token)
    umask 077
    printf '%s\n' \
        'SYBIL_LOCAL_API_PORT=3100' \
        'SYBIL_LOCAL_HISTORY_PORT=3103' \
        'SYBIL_LOCAL_WEB_PORT=3105' \
        'SYBIL_LOCAL_ARENA_PORT=8601' \
        'SYBIL_LOCAL_PROVER_PORT=3102' \
        'SYBIL_LOCAL_METRICS_PORT=8528' \
        'SYBIL_LOCAL_ALERTS_PORT=8980' \
        'SYBIL_LOCAL_GRAFANA_PORT=3101' \
        "SYBIL_SOAK_MM_TOKEN=$mm_token" \
        > "$ENV_FILE"
    {
        printf '%s\n' '{' '  "actors": ['
        printf '    {"principal_id":"mm","role":"market_maker","account_id":1,"token":"%s"},\n' "$mm_token"
        for index in $(seq 0 14); do
            account_id=$((index + 2))
            noise_token=$(random_token)
            comma=,
            if [[ "$index" -eq 14 ]]; then comma=; fi
            printf '    {"principal_id":"noise-%d","role":"noise","account_id":%d,"token":"%s"}%s\n' \
                "$index" "$account_id" "$noise_token" "$comma"
        done
        printf '%s\n' '  ]' '}'
    } > "$CREDENTIALS_FILE"
    echo "Generated local-only actor credentials in $STATE_DIR/"
}

compose() {
    local env_args=(--env-file "$ENV_FILE")
    if [[ -f arena.env ]]; then
        env_args+=(--env-file arena.env)
    fi
    if [[ -f arena/.env ]]; then
        env_args+=(--env-file arena/.env)
    fi
    docker compose \
        --project-name "$PROJECT" \
        "${env_args[@]}" \
        "${COMPOSE_FILES[@]}" \
        "$@"
}

require_openrouter_key() {
    if [[ -n "${OPENROUTER_API_KEY:-}" ]]; then
        return
    fi
    if [[ -f arena.env ]] && grep -Eq '^OPENROUTER_API_KEY=.+$' arena.env; then
        return
    fi
    if [[ -f arena/.env ]] && grep -Eq '^OPENROUTER_API_KEY=.+$' arena/.env; then
        return
    fi
    die "OPENROUTER_API_KEY is required for Arena; export it or add it to ignored arena.env/arena/.env"
}

print_urls() {
    cat <<'EOF'
Local Actor Liquidity soak is running:
  Frontend:       http://localhost:3105
  Liquidity view: http://localhost:3105/dev/overview
  Arena:          http://localhost:8601
  Grafana:        http://localhost:3101  (admin / sybil-dev)
  API:            http://localhost:3100

Initial mirror/native sync can take several minutes. Use:
  just local-soak-status
  just local-soak-logs
EOF
}

status() {
    require_local_docker
    [[ -f "$ENV_FILE" ]] || die "not prepared; run 'just local-soak-up'"
    compose ps
    echo
    echo "Liquidity health:"
    local health
    if ! health=$(curl -fsS --max-time 10 http://localhost:3100/v1/liquidity/health); then
        echo "  unavailable"
        return
    fi
    python3 -c '
import collections, json, sys
d = json.load(sys.stdin)
active = d.get("active_markets", 0)
noise_selected = d.get("noise_markets_selected")
if noise_selected is None:
    noise_selected = sum(1 for row in d.get("markets", []) if row.get("noise_orders", 0) > 0)
noise_coverage_bps = d.get("noise_coverage_bps")
if noise_coverage_bps is None:
    noise_coverage_bps = (noise_selected * 10_000 // active) if active else 0
print("  height={height} generation={universe_generation} active={active_markets}".format(**d))
print("  MM={mm_markets_quoted}/{active_markets} current ({:.2f}%); rolling two-sided={:.2f}%/{} blocks".format(d["mm_coverage_bps"] / 100, d.get("rolling_mm_two_sided_coverage_bps", 0) / 100, d.get("rolling_window_blocks", 0), **d))
print("  noise={}/{} actors; selected={}/{} current ({:.2f}%); rolling selected={:.2f}% naturally-marketable={:.2f}% filled={:.2f}%".format(d.get("observed_noise_actors", 0), d.get("expected_noise_actors", 0), noise_selected, active, noise_coverage_bps / 100, d.get("rolling_noise_coverage_bps", 0) / 100, d.get("rolling_noise_crossing_coverage_bps", 0) / 100, d.get("rolling_noise_fill_coverage_bps", 0) / 100))
print("  cleared={markets_with_clearing_prices}/{active_markets} fills={total_fills} rejections={total_rejections} volume_nanos={total_volume_nanos}".format(**d))
reasons = collections.Counter(
    row.get("mm_skip_reason") or "missing_without_reason"
    for row in d["markets"] if not row.get("mm_orders")
)
if reasons:
    print("  MM exceptions=" + ", ".join(f"{reason}:{count}" for reason, count in sorted(reasons.items())))
' <<<"$health"
}

case "${1:-}" in
    prepare)
        prepare
        compose config --quiet
        echo "Local soak configuration is ready."
        ;;
    up)
        prepare
        require_openrouter_key
        compose config --quiet
        # Several runtime services intentionally share one application image.
        # Build each tag once to avoid concurrent BuildKit exporters racing on
        # sybil-api:latest or sybil-arena:latest.
        compose build "${BUILD_SERVICES[@]}"
        compose up -d --no-build "${SERVICES[@]}"
        print_urls
        ;;
    status)
        status
        ;;
    logs)
        require_local_docker
        [[ -f "$ENV_FILE" ]] || die "not prepared; run 'just local-soak-up'"
        compose logs -f --tail=200 sybil-api sybil-polymarket sybil-arena
        ;;
    stop)
        require_local_docker
        [[ -f "$ENV_FILE" ]] || die "nothing to stop"
        compose down
        ;;
    clean)
        require_local_docker
        if [[ -f "$ENV_FILE" ]]; then
            compose down --volumes --remove-orphans
        else
            for volume in \
                sybil-local-soak-data \
                sybil-local-soak-history \
                sybil-local-soak-polymarket \
                sybil-local-soak-arena \
                sybil-local-soak-metrics \
                sybil-local-soak-prover-jobs \
                sybil-local-soak-prover-artifacts; do
                docker volume rm "$volume" >/dev/null 2>&1 || true
            done
        fi
        rm -rf "$STATE_DIR"
        echo "Removed only the $PROJECT containers, volumes, and generated credentials."
        ;;
    *)
        die "usage: $0 {prepare|up|status|logs|stop|clean}"
        ;;
esac
