#!/usr/bin/env bash
# Shared, source-only helpers for Sybil live smoke probes.

# Read a dotted path from JSON on stdin. Missing/invalid paths print nothing.
smoke_jget() {
    python3 -c '
import sys, json
path = sys.argv[1]
try:
    cur = json.load(sys.stdin)
except Exception:
    sys.exit(0)
for seg in (path.split(".") if path else []):
    if seg == "":
        continue
    try:
        if isinstance(cur, list):
            cur = cur[int(seg)]
        elif isinstance(cur, dict):
            cur = cur.get(seg)
        else:
            cur = None
    except Exception:
        cur = None
    if cur is None:
        break
if isinstance(cur, bool):
    print("true" if cur else "false")
elif cur is not None:
    print(cur)
' "$1"
}

smoke_is_2xx() { [[ "$1" =~ ^2[0-9][0-9]$ ]]; }

# A mature chain is ready for signed actions only when health exposes both a
# positive committed height and the lowercase 32-byte genesis domain.
smoke_is_committed_chain_identity() {
    local height=${1:-} genesis_hash=${2:-}
    [[ "$height" =~ ^[1-9][0-9]*$ && "$genesis_hash" =~ ^[0-9a-f]{64}$ ]]
}

# Summarize the public market registry for launch readiness. Prints:
#   OK <native> <mirrored> <mirrored-with-reference> <trade-market-id>
# and returns non-zero for malformed/non-array JSON.
smoke_market_inventory() {
    python3 -c '
import json
import sys

try:
    markets = json.load(sys.stdin)
except Exception:
    raise SystemExit(1)
if not isinstance(markets, list):
    raise SystemExit(1)

if not all(isinstance(market, dict) for market in markets):
    raise SystemExit(1)

def active(market):
    return str(market.get("status") or "").lower() == "active"

def valid_reference(market):
    try:
        value = int(market.get("reference_price_nanos"))
    except (TypeError, ValueError):
        return False
    return 0 < value <= 1_000_000_000

native = [
    market
    for market in markets
    if active(market)
    and market.get("polymarket_condition_id") is None
    and (market.get("resolution_criteria") or "") != ""
]
mirrored = [
    market
    for market in markets
    if active(market)
    and str(market.get("polymarket_condition_id") or "").strip() != ""
]
referenced = [market for market in mirrored if valid_reference(market)]
trade_candidates = [
    market
    for market in markets
    if active(market) and market.get("polymarket_condition_id") is None
] or markets
market_id = trade_candidates[0].get("market_id") if trade_candidates else None
print(
    "OK",
    len(native),
    len(mirrored),
    len(referenced),
    market_id if market_id is not None else "",
)
'
}

smoke_market_inventory_is_ready() {
    local status=${1:-} native=${2:-} mirrored=${3:-} referenced=${4:-}
    [[ "$status" == "OK" \
        && "$native" =~ ^[0-9]+$ && "$native" -ge 1 \
        && "$mirrored" =~ ^[0-9]+$ && "$mirrored" -ge 1 \
        && "$referenced" =~ ^[0-9]+$ && "$referenced" -ge 1 ]]
}

# Read one unlabeled Prometheus scalar from exposition text on stdin.
smoke_prometheus_scalar() {
    local metric=$1
    python3 -c '
import re
import sys

metric = sys.argv[1]
pattern = re.compile(r"^" + re.escape(metric) + r"[ \t]+([^ \t]+)[ \t]*$")
for raw in sys.stdin:
    match = pattern.match(raw.rstrip("\n"))
    if match:
        print(match.group(1))
        raise SystemExit(0)
raise SystemExit(1)
' "$metric"
}

smoke_reference_age_is_fresh() {
    local age=${1:-} max_age=${2:-}
    python3 - "$age" "$max_age" <<'PY'
import math
import sys

try:
    age = float(sys.argv[1])
    maximum = float(sys.argv[2])
except ValueError:
    raise SystemExit(1)
if not math.isfinite(age) or not math.isfinite(maximum):
    raise SystemExit(1)
raise SystemExit(0 if 0 <= age <= maximum else 1)
PY
}

# Parse /proofs/latest against a chain head and that height's canonical state
# root. Prints one of:
#   OK <proof-height> <lag>
#   STALE <proof-height> <lag>
#   ERR <reason>
smoke_proof_lag_result() {
    local chain_height=${1:-} max_lag=${2:-} canonical_state_root=${3:-}
    python3 -c '
import json
import re
import sys

try:
    chain_height = int(sys.argv[1])
    max_lag = int(sys.argv[2])
except (TypeError, ValueError):
    print("ERR invalid-chain-or-limit")
    raise SystemExit(1)
if chain_height < 0 or max_lag < 0:
    print("ERR invalid-chain-or-limit")
    raise SystemExit(1)

try:
    body = json.load(sys.stdin)
except Exception:
    print("ERR malformed-json")
    raise SystemExit(1)
if not isinstance(body, dict):
    print("ERR non-object")
    raise SystemExit(1)
proof_height = body.get("block_height")
if isinstance(proof_height, bool) or not isinstance(proof_height, int) or proof_height < 0:
    print("ERR invalid-block-height")
    raise SystemExit(1)
if proof_height > chain_height:
    print("ERR future-block-height")
    raise SystemExit(1)
if body.get("status") != "prepared":
    print("ERR invalid-worker-status")
    raise SystemExit(1)
proof_status = str(body.get("proof_status") or "").lower()
if not proof_status or proof_status == "not_started" or "fail" in proof_status or "error" in proof_status:
    print("ERR invalid-proof-status")
    raise SystemExit(1)

def root(value):
    normalized = str(value or "").lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    return normalized if re.fullmatch(r"[0-9a-f]{64}", normalized) else None

proof_root = root(body.get("state_root"))
canonical_root = root(sys.argv[3])
if proof_root is None:
    print("ERR invalid-proof-state-root")
    raise SystemExit(1)
if canonical_root is None:
    print("ERR invalid-canonical-state-root")
    raise SystemExit(1)
if proof_root != canonical_root:
    print("ERR state-root-mismatch")
    raise SystemExit(1)

lag = chain_height - proof_height
status = "OK" if lag <= max_lag else "STALE"
print(status, proof_height, lag)
raise SystemExit(0 if status == "OK" else 1)
' "$chain_height" "$max_lag" "$canonical_state_root"
}

# Run the same Docker command locally or through the post-deploy SSH hop.
# The command is intentionally a single string because the remote form is one
# SSH command. Callers must only pass repository-owned command text.
smoke_docker_run() {
    local docker_ssh=$1 command=$2
    if [[ -n "$docker_ssh" ]]; then
        ssh -o BatchMode=yes -o ConnectTimeout=10 "$docker_ssh" "$command" 2>/dev/null
    else
        eval "$command" 2>/dev/null
    fi
}

smoke_docker_available() {
    local docker_ssh=$1
    if [[ -n "$docker_ssh" ]]; then
        smoke_docker_run "$docker_ssh" "command -v docker" >/dev/null
    else
        command -v docker >/dev/null 2>&1
    fi
}

# GET one HTTP endpoint from a running Compose service container. Repository
# callers provide fixed service names and loopback URLs; no user input belongs
# in these command strings.
smoke_compose_service_curl() {
    local docker_ssh=$1 compose_project=$2 service=$3 url=$4 max_time=${5:-10}
    local command
    command="container=\$(docker ps -q --filter label=com.docker.compose.project=$compose_project --filter label=com.docker.compose.service=$service | head -1); test -n \"\$container\" && docker exec \"\$container\" curl -fsS --max-time $max_time $url"
    if [[ -n "$docker_ssh" ]]; then
        timeout "${max_time}s" ssh -o BatchMode=yes -o ConnectTimeout="$max_time" \
            "$docker_ssh" "$command" 2>/dev/null
    else
        timeout "${max_time}s" bash -c "$command" 2>/dev/null
    fi
}

# Print one row per compose container:
#   "<service> <name> <status> <health|none> <exit-code>".
smoke_compose_service_rows() {
    local docker_ssh=$1 compose_project=$2
    smoke_docker_run "$docker_ssh" \
        "docker ps -aq --filter label=com.docker.compose.project=$compose_project | xargs -r docker inspect --format '{{index .Config.Labels \"com.docker.compose.service\"}} {{.Name}} {{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}} {{.State.ExitCode}}'"
}

# Print one row per running compose container:
#   "<service> <restart-count> <oom-killed> <limit> <current> <peak>".
# RestartCount survives an automatic restart of the same container, which is
# the lifecycle history hidden by a point-in-time running/healthy check. The
# cgroup-v2 counters preserve the process's memory curve without exposing the
# Docker socket to the monitoring containers.
smoke_compose_resource_rows() {
    local docker_ssh=$1 compose_project=$2
    smoke_docker_run "$docker_ssh" \
        "docker ps -q --filter label=com.docker.compose.project=$compose_project | while read -r container; do state=\$(docker inspect --format '{{index .Config.Labels \"com.docker.compose.service\"}} {{.RestartCount}} {{.State.OOMKilled}} {{.HostConfig.Memory}}' \"\$container\") || exit; current=\$(docker exec \"\$container\" cat /sys/fs/cgroup/memory.current) || exit; peak=\$(docker exec \"\$container\" sh -c 'cat /sys/fs/cgroup/memory.peak 2>/dev/null || cat /sys/fs/cgroup/memory.current') || exit; printf '%s %s %s\\n' \"\$state\" \"\$current\" \"\$peak\"; done"
}

# Native catalog installation is an explicit one-shot deployment service. No
# other stopped container is healthy merely because it happened to exit zero.
smoke_service_allows_successful_completion() {
    [[ "${1:-}" == "sybil-native-admin" ]]
}

# Apply the shared compose health policy and invoke callbacks supplied by the
# caller. Callback signatures are callback "message".
smoke_check_compose_services() {
    local docker_ssh=$1 compose_project=$2 pass_cb=$3 fail_cb=$4 unavailable_cb=$5
    if ! smoke_docker_available "$docker_ssh"; then
        "$unavailable_cb" "docker unavailable ($([[ -n "$docker_ssh" ]] && echo "ssh $docker_ssh" || echo local)); container-health matrix needs an on-box run (SYBIL_SMOKE_DOCKER_SSH)"
        return
    fi

    local rows
    rows="$(smoke_compose_service_rows "$docker_ssh" "$compose_project")"
    if [[ -z "$rows" ]]; then
        "$fail_cb" "no containers found for compose project '$compose_project'"
        return
    fi

    local saw_api=0 service name status health exit_code
    while read -r service name status health exit_code; do
        [[ -z "$service" || -z "$name" ]] && continue
        name="${name#/}"
        [[ "$service" == "sybil-api" ]] && saw_api=1
        if [[ "$status" == "running" && ( "$health" == "none" || "$health" == "healthy" ) ]]; then
            "$pass_cb" "service $name: $status/$health"
        elif smoke_service_allows_successful_completion "$service" \
            && [[ "$status" == "exited" && "$exit_code" == "0" ]]; then
            "$pass_cb" "service $name: completed successfully (exit 0)"
        else
            "$fail_cb" "service $name: $status/$health exit=$exit_code (not healthy or expected successful completion)"
        fi
    done <<< "$rows"
    if [[ "$saw_api" -ne 1 ]]; then
        "$fail_cb" "required service sybil-api not found in project '$compose_project'"
    fi
}
