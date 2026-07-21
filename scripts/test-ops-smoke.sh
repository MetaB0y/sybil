#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT

mkdir -p "$tmpdir/bin"
cat > "$tmpdir/bin/docker" <<'FAKE_DOCKER'
#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
    ps)
        if [[ -n "${FAKE_CONTAINERS:-}" ]]; then
            printf '%s\n' "$FAKE_CONTAINERS"
        fi
        ;;
    inspect)
        format=${3:-}
        case "$format" in
            *HostConfig.NetworkMode*)
                printf '%s\n' "${FAKE_NETWORKS:-}"
                ;;
            *NetworkSettings.Ports*)
                printf '%s\n' "${FAKE_BINDINGS:-}"
                ;;
            *entrypoint=*)
                printf '%s\n' "${FAKE_COMMANDS:-}"
                ;;
            *)
                echo "unexpected docker inspect format: $format" >&2
                exit 2
                ;;
        esac
        ;;
    *)
        echo "unexpected docker command: $*" >&2
        exit 2
        ;;
esac
FAKE_DOCKER
chmod +x "$tmpdir/bin/docker"

run_smoke() {
    PATH="$tmpdir/bin:$PATH" scripts/ops-smoke.sh 2>&1
}

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

base_networks='/sybil-api sybil_default
/sybil-caddy sybil_default'
base_commands='/sybil-api entrypoint=[] cmd=["sybil-api"] path="sybil-api" args=[]'

if ! output=$(
    FAKE_CONTAINERS=$'api\ncaddy' \
    FAKE_NETWORKS="$base_networks" \
    FAKE_BINDINGS=$'/sybil-caddy 80/tcp 127.0.0.1 3108\n/sybil-caddy 443/tcp ::1 3143' \
    FAKE_COMMANDS="$base_commands" \
    run_smoke
); then
    fail "loopback-only Sybil project was rejected: $output"
fi
[[ "$output" == *"bindings are loopback-only"* ]] \
    || fail "loopback success was not reported"

set +e
output=$(
    FAKE_CONTAINERS=api \
    FAKE_NETWORKS='/sybil-api sybil_default' \
    FAKE_BINDINGS='/sybil-api 3000/tcp 0.0.0.0 3000' \
    FAKE_COMMANDS="$base_commands" \
    run_smoke
)
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"Unexpected public Sybil Docker exposure"* ]] \
    || fail "public Sybil binding did not fail closed"

set +e
output=$(
    FAKE_CONTAINERS=api \
    FAKE_NETWORKS='/sybil-api host' \
    FAKE_BINDINGS='' \
    FAKE_COMMANDS="$base_commands" \
    run_smoke
)
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"network_mode=host"* ]] \
    || fail "host-networked Sybil container did not fail closed"

secret='sk-or-test-secret'
set +e
output=$(
    FAKE_CONTAINERS=arena \
    FAKE_NETWORKS='/sybil-arena sybil_default' \
    FAKE_BINDINGS='' \
    FAKE_COMMANDS="/sybil-arena entrypoint=[] cmd=[\"--api-key=$secret\"]" \
    run_smoke
)
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"sk-or-REDACTED"* ]] \
    || fail "secret-like Sybil command argument did not fail closed and redact"
[[ "$output" != *"$secret"* ]] || fail "secret-like fixture was printed without redaction"

set +e
output=$(FAKE_CONTAINERS='' run_smoke)
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"No running containers found"* ]] \
    || fail "missing Sybil project did not fail closed"

echo "ops smoke tests: ok"
