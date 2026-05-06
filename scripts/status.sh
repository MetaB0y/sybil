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
LOGS=$(ssh "$S" 'cd /opt/sybil && docker compose -f docker-compose.yml -f docker-compose.prod.yml logs --tail 1000 sybil-arena 2>&1' | grep -v httpx)
count_logs() {
    local pattern="$1"
    echo "$LOGS" | grep -c "$pattern" || true
}
echo "  LLM calls:      $(count_logs 'LLM response')"
echo "  Parse failures: $(count_logs 'Failed to parse')"
echo "  Trade orders:   $(count_logs 'Buy')"
echo "  HOLDs:          $(count_logs 'HOLD')"
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
echo "  Polls:          $(count_logs 'Poll:')"
echo "  Articles gated: $(count_logs '✓')"
echo ""

echo "=== Pending Orders ==="
ssh "$S" 'python3 - <<'"'"'PY'"'"'
import collections, json, urllib.request
try:
    orders = json.load(urllib.request.urlopen("http://localhost:3000/v1/orders/pending", timeout=5))
except Exception as exc:
    print(f"  unavailable: {exc}")
    raise SystemExit

print(f"  Total:          {len(orders)}")
if not orders:
    raise SystemExit

by_account = collections.Counter(o["account_id"] for o in orders)
by_market = collections.Counter(o["market_id"] for o in orders)
created_min = min(o["created_at_block"] for o in orders)
created_max = max(o["created_at_block"] for o in orders)
expiry_min = min(o["expires_at_block"] for o in orders)
expiry_max = max(o["expires_at_block"] for o in orders)
gtc_like = sum(
    1 for o in orders
    if o["expires_at_block"] - o["created_at_block"] > 1_000_000
)

print(f"  Top accounts:   {by_account.most_common(5)}")
print(f"  Top markets:    {by_market.most_common(5)}")
print(f"  Created range:  {created_min}..{created_max}")
print(f"  Expiry range:   {expiry_min}..{expiry_max}")
print(f"  GTC-like:       {gtc_like}")
PY'
echo ""

echo "=== Recent Decisions ==="
echo "$LOGS" | grep 'FV=' | tail -10
