#!/usr/bin/env bash
# Render every maintained Mermaid block with the pinned official CLI image.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${MERMAID_CLI_IMAGE:-ghcr.io/mermaid-js/mermaid-cli/mermaid-cli:11.15.0}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cd "$ROOT"

mapfile -d '' FILES < <(
  rg -l -0 '^```mermaid' . \
    --glob '*.md' \
    --glob '!design/archive/**' \
    --glob '!frontend/archive/**' \
    --glob '!site/**' \
    --glob '!target/**' \
    --glob '!**/node_modules/**'
)

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "No maintained Mermaid diagrams found"
  exit 0
fi

perl -0777 -ne '
  while (/```mermaid\s*\n(.*?)```/sg) {
    print "```mermaid\n$1```\n\n";
  }
' "${FILES[@]}" > "$TMP/diagrams.md"

COUNT="$(rg -c '^```mermaid' "$TMP/diagrams.md")"
chmod 0777 "$TMP"
docker run --rm \
  -v "$TMP:/data" \
  "$IMAGE" \
  -i /data/diagrams.md -o /data/rendered.md >/dev/null

echo "Mermaid render check passed: $COUNT diagrams"
