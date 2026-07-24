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

fake_working_revision=""
fake_parent_revision="$revision"
fake_main_revision="$revision"
jj() {
    local rev=""
    while [[ $# -gt 0 ]]; do
        if [[ "$1" == "-r" ]]; then
            rev="$2"
            shift 2
        else
            shift
        fi
    done
    case "$rev" in
        '@ & ~empty()') printf '%s' "$fake_working_revision" ;;
        '@-') printf '%s\n' "$fake_parent_revision" ;;
        'main@origin') printf '%s\n' "$fake_main_revision" ;;
        *) return 2 ;;
    esac
}

# A content-empty merge commit is still a valid main revision. The release
# source is the direct parent of the required empty working change, not the
# latest ancestor that happens to change file content.
[[ "$(source_revision)" == "$revision" ]]
fake_working_revision=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
if (source_revision >"$work/nonempty.out" 2>"$work/nonempty.err"); then
    echo "nonempty jj change passed release source validation" >&2
    exit 1
fi
grep -q 'start a new empty jj change' "$work/nonempty.err"
fake_working_revision=""
fake_main_revision=cccccccccccccccccccccccccccccccccccccccc
if (source_revision >"$work/diverged.out" 2>"$work/diverged.err"); then
    echo "unpushed jj revision passed release source validation" >&2
    exit 1
fi
grep -q 'is not the pushed main@origin' "$work/diverged.err"
fake_main_revision="$revision"

portable_a="$(
    image_runtime_fingerprint <<'JSON'
[{
  "Id": "sha256:local-engine-id",
  "RepoTags": ["sybil-api:test"],
  "RepoDigests": [],
  "Architecture": "amd64",
  "Os": "linux",
  "Created": "2026-07-20T00:00:00Z",
  "Config": {"Labels": {"org.opencontainers.image.revision": "aaaaaaaa"}},
  "RootFS": {"Type": "layers", "Layers": ["sha256:layer"]},
  "GraphDriver": {"Name": "overlay2"},
  "Size": 100
}]
JSON
)"
portable_b="$(
    image_runtime_fingerprint <<'JSON'
[{
  "Id": "sha256:remote-engine-id",
  "RepoTags": ["sybil-api:test"],
  "RepoDigests": ["sybil-api@sha256:manifest"],
  "Architecture": "amd64",
  "Os": "linux",
  "Created": "2026-07-20T00:00:00Z",
  "Config": {"Labels": {"org.opencontainers.image.revision": "aaaaaaaa"}},
  "RootFS": {"Type": "layers", "Layers": ["sha256:layer"]},
  "Size": 200
}]
JSON
)"
portable_changed="$(
    image_runtime_fingerprint <<'JSON'
[{
  "Architecture": "amd64",
  "Os": "linux",
  "Created": "2026-07-20T00:00:00Z",
  "Config": {"Labels": {"org.opencontainers.image.revision": "bbbbbbbb"}},
  "RootFS": {"Type": "layers", "Layers": ["sha256:layer"]}
}]
JSON
)"
[[ "$portable_a" == "$portable_b" ]]
[[ "$portable_a" != "$portable_changed" ]]

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
    $'sybil-api\tsha256:one\trunning\nsybil-history\tsha256:one\trunning\n'

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
assert set(record["verified_containers"]) == {"sybil-api", "sybil-history"}
assert record["verified_containers"]["sybil-api"] == {
    "image_id": "sha256:one",
    "state": "running",
}
PY

api_services="$(services_for_scope api)"
grep -qx sybil-api <<<"$api_services"
grep -qx sybil-history <<<"$api_services"
grep -qx sybil-native-admin <<<"$api_services"
grep -qx sybil-native-mm <<<"$api_services"
grep -qx sybil-polymarket <<<"$api_services"

verified_services="$(
    ssh() {
        [[ "$1" == "-n" ]] || {
            echo "read-only SSH call can consume a service-list loop" >&2
            return 2
        }
        shift
        local server="$1" command="$2"
        [[ "$server" == "test-host" ]]
        if [[ "$command" == docker\ ps* ]]; then
            sed -n "s/.*service=\\([^']*\\)'.*/cid-\\1/p" <<<"$command"
        elif [[ "$command" == *".State.Status"* ]]; then
            if [[ "$command" == *"cid-sybil-native-admin"* ]]; then
                printf '%s\n' "exited:0"
            else
                printf '%s\n' "running:0"
            fi
        else
            printf '%s\n' "$image_id"
        fi
    }
    export -f ssh
    verify_containers test-host api "$image_id" "$image_id" "$image_id" "$image_id"
)"
for service in sybil-api sybil-history sybil-native-admin sybil-native-mm sybil-polymarket; do
    if [[ "$service" == "sybil-native-admin" ]]; then
        state="exited:0"
    else
        state="running"
    fi
    grep -qx "${service}"$'\t'"${image_id}"$'\t'"${state}" <<<"$verified_services"
done
[[ "$(wc -l <<<"$verified_services")" -eq 5 ]]

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
