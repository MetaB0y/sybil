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
for command in bash cat curl date dirname grep head mktemp python3 rm sed sleep awk; do
    ln -s "$(command -v "$command")" "$tmpdir/bin/$command"
done

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

start_fixture() {
    local mode=$1 port_file="$tmpdir/port"
    rm -f "$port_file"
    python3 scripts/_synthetic_probe_fixture.py --mode "$mode" --port-file "$port_file" &
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

echo "synthetic probe tests: ok"
