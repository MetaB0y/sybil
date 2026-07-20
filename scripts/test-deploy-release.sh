#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=deploy-release.sh
source "$ROOT/scripts/deploy-release.sh"

work="$(mktemp -d "${TMPDIR:-/tmp}/sybil-release-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT

revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
api_ref="sybil-api:$revision"
arena_ref="sybil-arena:$revision"
web_ref="sybil-web:$revision"
image_id="sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

write_images_env "$work/images.env" "$api_ref" "$arena_ref" "$web_ref"
env_body="$(cat "$work/images.env")"
[[ "$(env_ref "$env_body" SYBIL_API_IMAGE)" == "$api_ref" ]]
[[ "$(env_ref "$env_body" SYBIL_ARENA_IMAGE)" == "$arena_ref" ]]
[[ "$(env_ref "$env_body" SYBIL_WEB_IMAGE)" == "$web_ref" ]]
[[ "$(env_ref "$env_body" SYBIL_CADDY_IMAGE)" == "$CADDY_REF" ]]

write_manifest "$work/manifest.json" test-release \
    2026-07-20T00:00:00Z 2026-07-20T00:01:00Z all "$revision" \
    "$api_ref" "$arena_ref" "$web_ref" \
    "$image_id" "$revision" "$image_id" "$revision" \
    "$image_id" "$revision" "$image_id" \
    $'sybil-api=sha256:one\nsybil-history=sha256:one\n'

python3 - "$work/manifest.json" "$revision" "$CADDY_REF" <<'PY'
import json
import sys

path, revision, caddy_ref = sys.argv[1:]
record = json.load(open(path, encoding="utf-8"))
assert record["schema"] == "sybil.release.v1"
assert record["status"] == "verified"
assert record["promotion_source_revision"] == revision
assert record["images"]["sybil-api"]["source_revision"] == revision
assert record["images"]["caddy"]["ref"] == caddy_ref
assert set(record["running_containers"]) == {"sybil-api", "sybil-history"}
PY

api_services="$(services_for_scope api)"
grep -qx sybil-api <<<"$api_services"
grep -qx sybil-history <<<"$api_services"
grep -qx sybil-native-admin <<<"$api_services"
grep -qx sybil-native-mm <<<"$api_services"
grep -qx sybil-polymarket <<<"$api_services"

rollback_body="$(
    awk '
        /^rollback\(\)/ { in_body = 1 }
        in_body && /^verify_current\(\)/ { exit }
        in_body { print }
    ' "$ROOT/scripts/deploy-release.sh"
)"
if grep -Eq 'build_image|docker[ -]compose build|docker build' <<<"$rollback_body"; then
    echo "rollback path can rebuild" >&2
    exit 1
fi

echo "deploy-release tests: ok"
