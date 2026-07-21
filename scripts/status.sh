#!/usr/bin/env bash
set -euo pipefail

S=${SYBIL_STATUS_SSH:-patty}
COMPOSE="docker compose --env-file .env --env-file releases/current.env -f docker-compose.yml -f docker-compose.prod.yml --profile integrations --profile ops"

remote_api_get() {
    local path=$1
    ssh "$S" "cd /opt/sybil && $COMPOSE exec -T sybil-api curl -fsS --max-time 15 'http://127.0.0.1:3000${path}'"
}

remote_alerts_get() {
    # ALERTS is persisted for history through remote write, but its last firing
    # sample remains queryable briefly after resolution. Use vmalert's live API
    # for the operator-facing current-state view.
    ssh "$S" "cd /opt/sybil && $COMPOSE exec -T vmalert wget -qO- 'http://127.0.0.1:8880/api/v1/alerts'"
}

echo "=== Containers ==="
ssh "$S" 'docker ps -aq --filter label=com.docker.compose.project=sybil | while read -r container; do row=$(docker inspect --format "{{index .Config.Labels \"com.docker.compose.service\"}}|{{.State.Status}}|{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}|{{.RestartCount}}|{{.State.OOMKilled}}|{{.HostConfig.Memory}}|{{.State.StartedAt}}" "$container") || exit; if [ "$(docker inspect --format "{{.State.Running}}" "$container")" = true ]; then current=$(docker exec "$container" cat /sys/fs/cgroup/memory.current) || exit; peak=$(docker exec "$container" sh -c "cat /sys/fs/cgroup/memory.peak 2>/dev/null || cat /sys/fs/cgroup/memory.current") || exit; else current=-; peak=-; fi; printf "%s|%s|%s\n" "$row" "$current" "$peak"; done' \
    | sort \
    | awk -F'|' '
        function human(n, value, unit) {
            if (n == "-" || n == 0) return n
            value = n
            unit = "B"
            if (value >= 1073741824) { value /= 1073741824; unit = "GiB" }
            else if (value >= 1048576) { value /= 1048576; unit = "MiB" }
            else if (value >= 1024) { value /= 1024; unit = "KiB" }
            return sprintf("%.1f%s", value, unit)
        }
        BEGIN { printf "%-24s %-10s %-10s %-8s %-5s %-10s %-10s %-10s %s\n", "SERVICE", "STATUS", "HEALTH", "RESTARTS", "OOM", "MEMORY", "PEAK", "LIMIT", "STARTED_UTC" }
        { printf "%-24s %-10s %-10s %-8s %-5s %-10s %-10s %-10s %s\n", $1, $2, $3, $4, $5, human($8), human($9), human($6), $7 }
    '
echo ""

echo "=== Alerts ==="
if alerts=$(remote_alerts_get); then
    python3 -c '
import json
import sys

payload = json.load(sys.stdin)
results = payload.get("data", {}).get("alerts")
if payload.get("status") != "success" or not isinstance(results, list):
    raise SystemExit("vmalert returned an invalid active-alert response")
active = sorted(
    (
        sample.get("state", "unknown"),
        sample.get("labels", {}).get("severity", "unknown"),
        sample.get("name") or sample.get("labels", {}).get("alertname", "unnamed"),
        sample.get("labels", {}).get("component", "unknown"),
    )
    for sample in results
)
if not active:
    print("  none firing or pending")
for state, severity, name, component in active:
    print(f"  {state.upper():7} {severity:<8} {name} ({component})")
' <<<"$alerts"
else
    echo "  unavailable"
fi
echo ""

echo "=== Recent Blocks ==="
if blocks=$(remote_api_get '/v1/blocks?limit=4'); then
    python3 -c '
import json
import sys

for block in json.load(sys.stdin):
    marker = " <<<" if block["fill_count"] > 0 else ""
    # Protocol nanos are exact decimal strings on the JSON boundary.
    welfare = int(block["total_welfare_nanos"]) / 1_000_000_000
    print(
        f"  Block {block['"'"'height'"'"']}: {block['"'"'order_count'"'"']} orders, "
        f"{block['"'"'fill_count'"'"']} fills, {block['"'"'rejection_count'"'"']} rej, "
        f"welfare=${welfare:.2f}{marker}"
    )
' <<<"$blocks"
else
    echo "  unavailable"
fi
echo ""

echo "=== LLM Strategy Activity ==="
LOGS=$(ssh "$S" "cd /opt/sybil && $COMPOSE logs --tail 1000 sybil-arena 2>&1" | grep -v httpx || true)
count_logs() {
    local pattern=$1
    grep -c "$pattern" <<<"$LOGS" || true
}
echo "  LLM calls:      $(count_logs 'LLM response')"
echo "  Provider fails: $(count_logs 'LLM provider failure')"
echo "  Parse failures: $(count_logs 'Failed to parse')"
echo "  Trade decisions: $(count_logs 'Buy')"
echo "  HOLD decisions:  $(count_logs 'HOLD')"
echo ""

echo "=== News Pipeline ==="
echo "  Polls:          $(count_logs 'Poll:')"
echo "  Articles gated: $(count_logs '✓')"
echo ""

echo "=== Pending Orders ==="
if metrics=$(remote_api_get '/metrics'); then
    pending=$(awk '$1 == "sybil_pending_orders" { print $2; found = 1 } END { if (!found) print "not yet reported" }' <<<"$metrics")
    echo "  Total:          $pending"
else
    echo "  unavailable"
fi
echo ""

echo "=== Arena ==="
ssh "$S" "cd /opt/sybil && $COMPOSE exec -T -e ARENA_METRICS_URL=http://sybil-arena:9101/metrics sybil-arena-dashboard .venv/bin/python -m live.status --hours 24"
