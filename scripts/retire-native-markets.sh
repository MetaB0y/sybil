#!/usr/bin/env bash
#
# Retire the CURRENT sybil-native markets by flagging them `closed` (the frontend
# hides closed markets). Mirrored Polymarket markets are never touched.
#
# Run this BEFORE deploying the new native catalog, while the only native markets
# present are the ones being retired. Native markets are identified by the
# "native" create-time tag and/or a `native:` event id; that is the same
# mirror/native discriminator the backend uses (mirrored markets carry a
# polymarket_condition_id and a non-native event id).
#
# Usage:
#   SYBIL_URL=https://<api-host> SYBIL_SERVICE_TOKEN=<token> scripts/retire-native-markets.sh          # dry run: list only
#   SYBIL_URL=https://<api-host> SYBIL_SERVICE_TOKEN=<token> APPLY=1 scripts/retire-native-markets.sh   # actually close them
#
# Requires: curl, jq. The service token is the prod SYBIL_SERVICE_TOKEN
# (bearer-authed; same token the mirror uses for /metadata writes).
set -euo pipefail
: "${SYBIL_URL:?set SYBIL_URL (e.g. https://api.example.com)}"
: "${SYBIL_SERVICE_TOKEN:?set SYBIL_SERVICE_TOKEN}"
APPLY="${APPLY:-0}"
LIMIT="${LIMIT:-2000}"

NATIVE_FILTER='select(((.tags // []) | index("native")) or ((.event_id // "") | startswith("native:")))'

echo "Fetching markets from ${SYBIL_URL} ..."
markets_json="$(curl -fsS "${SYBIL_URL}/v1/markets?limit=${LIMIT}")"

native_rows="$(printf '%s' "$markets_json" | jq -r "(.markets // .) | .[]? | ${NATIVE_FILTER} | \"\(.market_id)\t\(.name)\"")"

if [ -z "$native_rows" ]; then
  echo "No native markets found. Nothing to retire."
  exit 0
fi

count="$(printf '%s\n' "$native_rows" | grep -c .)"
echo "Found ${count} native market(s):"
printf '%s\n' "$native_rows" | sed 's/^/  #/; s/\t/  /'

if [ "$APPLY" != "1" ]; then
  echo
  echo "DRY RUN — nothing changed. Re-run with APPLY=1 to close these markets."
  exit 0
fi

echo
echo "Closing ${count} native market(s) ..."
printf '%s\n' "$native_rows" | while IFS=$'\t' read -r id name; do
  [ -z "$id" ] && continue
  curl -fsS -X POST "${SYBIL_URL}/v1/markets/${id}/metadata" \
    -H "Authorization: Bearer ${SYBIL_SERVICE_TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"closed": true}' >/dev/null
  echo "  closed #${id}  ${name}"
done
echo "Done. ${count} native market(s) are now hidden from the frontend."
echo "Note: closing hides them; it does not settle open positions/orders. To fully"
echo "settle, resolve each market via the signed operator resolution path."
