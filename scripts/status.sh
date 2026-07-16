#!/usr/bin/env bash
set -euo pipefail

S=${SYBIL_STATUS_SSH:-root@172.104.31.54}
COMPOSE="docker compose -f docker-compose.yml -f docker-compose.prod.yml --profile integrations --profile validity --profile ops"

remote_api_get() {
    local path=$1
    ssh "$S" "cd /opt/sybil && $COMPOSE exec -T sybil-api curl -fsS --max-time 15 'http://127.0.0.1:3000${path}'"
}

echo "=== Containers ==="
ssh "$S" 'docker ps --format "table {{.Names}}\t{{.Status}}"'
echo ""

echo "=== Recent Blocks ==="
if blocks=$(remote_api_get '/v1/blocks?limit=4'); then
    python3 -c '
import json
import sys

for block in json.load(sys.stdin):
    marker = " <<<" if block["fill_count"] > 0 else ""
    welfare = block["total_welfare_nanos"] / 1_000_000_000
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

echo "=== Trader Activity ==="
LOGS=$(ssh "$S" "cd /opt/sybil && $COMPOSE logs --tail 1000 sybil-arena 2>&1" | grep -v httpx || true)
count_logs() {
    local pattern=$1
    grep -c "$pattern" <<<"$LOGS" || true
}
echo "  LLM calls:      $(count_logs 'LLM response')"
echo "  Parse failures: $(count_logs 'Failed to parse')"
echo "  Trade orders:   $(count_logs 'Buy')"
echo "  HOLDs:          $(count_logs 'HOLD')"
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
ssh "$S" "cd /opt/sybil && $COMPOSE exec -T sybil-arena-dashboard .venv/bin/python -m live.status --hours 24"
