#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
fixture_pid=""
cleanup() {
    if [[ -n "$fixture_pid" ]]; then
        kill "$fixture_pid" 2>/dev/null || true
        wait "$fixture_pid" 2>/dev/null || true
    fi
    rm -rf "$tmpdir"
}
trap cleanup EXIT

# Hide Docker deliberately. The scheduled probe treats Docker as an optional
# on-box extension, while this fixture isolates its public HTTP contract.
mkdir -p "$tmpdir/bin"
for command in bash cat curl date dirname grep head mktemp python3 rm sed sleep awk xargs; do
    ln -s "$(command -v "$command")" "$tmpdir/bin/$command"
done

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

start_fixture() {
    local mode=$1 port_file="$tmpdir/port" metrics_file="$tmpdir/metrics"
    rm -f "$port_file"
    : > "$metrics_file"
    python3 scripts/_synthetic_probe_fixture.py \
        --mode "$mode" --port-file "$port_file" --metrics-file "$metrics_file" &
    fixture_pid=$!
    for _ in $(seq 1 100); do
        [[ -s "$port_file" ]] && break
        sleep 0.02
    done
    [[ -s "$port_file" ]] || fail "fixture did not publish its port"
    fixture_port="$(<"$port_file")"
}

stop_fixture() {
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
    fixture_pid=""
}

run_probe() {
    PATH="$tmpdir/bin" \
    SYBIL_SYNTHETIC_VM_URL="http://127.0.0.1:$fixture_port/vm" \
    scripts/synthetic-probe.sh \
        --base-url "http://127.0.0.1:$fixture_port/api" \
        --app-origin "http://127.0.0.1:$fixture_port/app" \
        --block-interval 1 \
        --proof-lag off 2>&1
}

start_fixture ok
output="$(run_probe)" || fail "healthy public web path failed: $output"
[[ "$output" == *"OK: public web app"* ]] || fail "success omitted the web journey: $output"
stop_fixture

start_fixture app-http
set +e
output="$(run_probe)"
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"returned HTTP 503"* ]] \
    || fail "public app HTTP failure did not fail closed: $output"
stop_fixture

start_fixture app-shell
set +e
output="$(run_probe)"
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"did not return the Sybil app shell"* ]] \
    || fail "wrong public app shell did not fail closed: $output"
stop_fixture

start_fixture app-asset
set +e
output="$(run_probe)"
status=$?
set -e
[[ "$status" -ne 0 && "$output" == *"Next.js asset returned 503"* ]] \
    || fail "broken public Next.js asset did not fail closed: $output"
stop_fixture

cat > "$tmpdir/bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
    ps)
        echo fixture-container
        ;;
    inspect)
        case "$*" in
            *'.State.ExitCode'*)
                echo 'sybil-api /fixture-sybil-api-1 running healthy 0'
                ;;
            *'.State.StartedAt'*)
                echo 'sybil-api 0 false 134217728 2026-07-21T18:03:14.921882519Z'
                ;;
            *)
                exit 2
                ;;
        esac
        ;;
    exec)
        if [[ "${3:-}" == cat && "${4:-}" == /sys/fs/cgroup/memory.current ]]; then
            echo 1048576
        elif [[ "${3:-}" == sh ]]; then
            echo 2097152
        else
            exit 2
        fi
        ;;
    *)
        exit 2
        ;;
esac
EOF
chmod +x "$tmpdir/bin/docker"

start_fixture ok
output="$(run_probe)" || fail "healthy Docker resource path failed: $output"
grep -Fq 'sybil_synthetic_container_started_at_seconds{' "$tmpdir/metrics" \
    || fail "container start-time metric was not pushed"
grep -Fq 'service="sybil-api"} 1784656994' "$tmpdir/metrics" \
    || fail "container start time was not converted to epoch seconds"
stop_fixture

echo "synthetic probe tests: ok"
