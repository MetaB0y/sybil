#!/usr/bin/env bash
set -e

S="root@172.104.31.54"

echo "=== Containers ==="
ssh "$S" 'docker ps --format "table {{.Names}}\t{{.Status}}"'
echo ""

echo "=== Recent Blocks ==="
ssh "$S" 'timeout 8 curl -sN http://localhost:3000/v1/blocks/stream 2>/dev/null' | head -4 | while read -r line; do
    echo "$line" | python3 -c "
import sys, json
for l in sys.stdin:
    l = l.strip()
    if l.startswith('data: '):
        b = json.loads(l[6:])
        f = '<<<' if b['fill_count'] > 0 else ''
        print(f'  Block {b[\"height\"]}: {b[\"order_count\"]} orders, {b[\"fill_count\"]} fills, {len(b[\"rejections\"])} rej, welfare=\${b[\"total_welfare_nanos\"]/1e9:.2f} {f}')
" 2>/dev/null
done
echo ""

echo "=== Trader Activity ==="
LOGS=$(ssh "$S" 'docker logs sybil-arena 2>&1' | grep -v httpx)
echo "  LLM calls:      $(echo "$LOGS" | grep -c 'LLM response' || echo 0)"
echo "  Parse failures: $(echo "$LOGS" | grep -c 'Failed to parse' || echo 0)"
echo "  Trade orders:   $(echo "$LOGS" | grep -c 'Buy' || echo 0)"
echo "  HOLDs:          $(echo "$LOGS" | grep -c 'HOLD' || echo 0)"
echo ""

echo "=== Balances ==="
for id in 11 12 13; do
    ssh "$S" "curl -s http://localhost:3000/v1/accounts/$id" 2>/dev/null | python3 -c "
import sys, json
try:
    a = json.load(sys.stdin)
    print(f'  Account {a[\"id\"]}: \${a[\"balance_nanos\"]/1e9:.2f}')
except Exception:
    pass
"
done
echo ""

echo "=== News Pipeline ==="
echo "  Polls:          $(echo "$LOGS" | grep -c 'Poll:' || echo 0)"
echo "  Articles gated: $(echo "$LOGS" | grep -c '✓' || echo 0)"
echo ""

echo "=== Recent Decisions ==="
echo "$LOGS" | grep 'FV=' | tail -10
