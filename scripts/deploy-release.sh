#!/usr/bin/env bash
# Build, activate, verify, and roll back immutable Sybil application releases.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RECORD_DIR="$ROOT/deploy/releases"
CADDY_REF="caddy:2.11.4-alpine@sha256:5f5c8640aae01df9654968d946d8f1a56c497f1dd5c5cda4cf95ab7c14d58648"
SOURCE_LABEL="org.opencontainers.image.revision"
TMP=""

cleanup() {
    [[ -z "$TMP" ]] || rm -rf "$TMP"
}
trap cleanup EXIT

die() {
    echo "deploy-release: $*" >&2
    exit 2
}

info() {
    echo "deploy-release: $*"
}

usage() {
    cat >&2 <<'EOF'
usage:
  scripts/deploy-release.sh promote <api|arena|web|all> <ssh-host>
  scripts/deploy-release.sh rollback <release-id> <ssh-host> CONFIRM
  scripts/deploy-release.sh verify <ssh-host>

Promotion requires a new empty Jujutsu change whose direct parent equals
main@origin. Successful records are written to deploy/releases/.
Rollback reuses an already recorded host image set and never builds.
EOF
    exit "${1:-2}"
}

valid_release_id() {
    [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]
}

valid_image_ref() {
    [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._/@:+-]*$ ]]
}

local_compose() {
    if docker compose version >/dev/null 2>&1; then
        docker compose "$@"
    elif command -v docker-compose >/dev/null 2>&1; then
        docker-compose "$@"
    else
        die "docker compose or docker-compose is required"
    fi
}

source_revision() {
    local working_revision revision main
    working_revision="$(jj log -r '@ & ~empty()' --no-graph -T 'commit_id ++ "\n"')"
    [[ -z "$working_revision" ]] \
        || die "start a new empty jj change before building a release"
    revision="$(jj log -r '@-' --no-graph -T 'commit_id ++ "\n"')"
    main="$(jj log -r 'main@origin' --no-graph -T 'commit_id ++ "\n"')"
    [[ "$revision" =~ ^[0-9a-f]{40}$ ]] || die "could not derive a 40-hex source revision"
    [[ "$revision" == "$main" ]] \
        || die "source revision $revision is not the pushed main@origin $main"
    printf '%s\n' "$revision"
}

env_ref() {
    local body="$1" key="$2" value
    value="$(sed -n "s/^${key}=//p" <<<"$body")"
    [[ "$(wc -l <<<"$value")" -eq 1 ]] || die "release env must contain exactly one $key"
    valid_image_ref "$value" || die "release env contains invalid $key"
    printf '%s\n' "$value"
}

local_image_id() {
    docker image inspect --format '{{.Id}}' "$1"
}

image_runtime_fingerprint() {
    python3 -c '
import hashlib
import json
import sys

images = json.load(sys.stdin)
if len(images) != 1:
    raise SystemExit("expected exactly one inspected image")
image = images[0]
portable = {
    key: image.get(key)
    for key in ("Architecture", "Author", "Config", "Created", "Os", "RootFS", "Variant")
}
encoded = json.dumps(
    portable,
    ensure_ascii=True,
    separators=(",", ":"),
    sort_keys=True,
).encode()
print("sha256:" + hashlib.sha256(encoded).hexdigest())
'
}

local_image_fingerprint() {
    docker image inspect "$1" | image_runtime_fingerprint
}

local_image_revision() {
    docker image inspect --format "{{index .Config.Labels \"$SOURCE_LABEL\"}}" "$1"
}

remote_image_id() {
    local server="$1" ref="$2"
    ssh -n "$server" "docker image inspect --format='{{.Id}}' '$ref' 2>/dev/null" || true
}

remote_image_fingerprint() {
    local server="$1" ref="$2"
    ssh -n "$server" "docker image inspect '$ref' 2>/dev/null" \
        | image_runtime_fingerprint 2>/dev/null \
        || true
}

remote_image_revision() {
    local server="$1" ref="$2"
    ssh -n "$server" \
        "docker image inspect --format='{{index .Config.Labels \"$SOURCE_LABEL\"}}' '$ref' 2>/dev/null" \
        || true
}

build_image() {
    local service="$1" env_name="$2" ref="$3" revision="$4"
    local existing_revision
    if docker image inspect "$ref" >/dev/null 2>&1; then
        existing_revision="$(local_image_revision "$ref")"
        [[ "$existing_revision" == "$revision" ]] \
            || die "immutable local tag $ref already names revision $existing_revision"
        info "reusing local immutable image $ref"
        return
    fi

    info "building $ref from $revision"
    env "$env_name=$ref" DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 \
        DOCKER_DEFAULT_PLATFORM=linux/amd64 \
        bash -c '
            set -euo pipefail
            if docker compose version >/dev/null 2>&1; then
                docker compose build --build-arg "SYBIL_SOURCE_REVISION=$1" "$2"
            else
                docker-compose build --build-arg "SYBIL_SOURCE_REVISION=$1" "$2"
            fi
        ' _ "$revision" "$service"

    existing_revision="$(local_image_revision "$ref")"
    [[ "$existing_revision" == "$revision" ]] \
        || die "$ref was built without the expected source-revision label"
}

transfer_image() {
    local server="$1" ref="$2" local_id remote_id expected actual
    local_id="$(local_image_id "$ref")"
    expected="$(local_image_fingerprint "$ref")"
    remote_id="$(remote_image_id "$server" "$ref")"
    if [[ -n "$remote_id" ]]; then
        actual="$(remote_image_fingerprint "$server" "$ref")"
        [[ "$actual" == "$expected" ]] \
            || die "immutable remote tag $ref has runtime fingerprint $actual ($remote_id), expected $expected ($local_id)"
        info "reusing remote immutable image $ref ($actual; host id $remote_id)"
        return
    fi
    info "transferring $ref ($expected; local id $local_id)"
    docker save "$ref" | ssh "$server" docker load
    remote_id="$(remote_image_id "$server" "$ref")"
    actual="$(remote_image_fingerprint "$server" "$ref")"
    [[ "$actual" == "$expected" ]] \
        || die "remote load did not preserve $ref runtime fingerprint ($actual != $expected)"
    info "verified transferred $ref as host image $remote_id"
}

ensure_remote_caddy() {
    local server="$1" id
    id="$(remote_image_id "$server" "$CADDY_REF")"
    if [[ -z "$id" ]]; then
        info "pulling pinned Caddy image on the host"
        ssh "$server" "docker pull '$CADDY_REF' >/dev/null"
    fi
}

write_images_env() {
    local path="$1" api="$2" arena="$3" web="$4"
    {
        printf 'SYBIL_API_IMAGE=%s\n' "$api"
        printf 'SYBIL_ARENA_IMAGE=%s\n' "$arena"
        printf 'SYBIL_WEB_IMAGE=%s\n' "$web"
        printf 'SYBIL_CADDY_IMAGE=%s\n' "$CADDY_REF"
    } >"$path"
}

write_manifest() {
    local path="$1" release_id="$2" created="$3" verified="$4" scope="$5"
    local promotion_revision="$6" api_ref="$7" arena_ref="$8" web_ref="$9"
    shift 9
    local api_id="$1" api_revision="$2" arena_id="$3" arena_revision="$4"
    local web_id="$5" web_revision="$6" caddy_id="$7" containers="${8:-}"
    python3 - "$path" "$release_id" "$created" "$verified" "$scope" \
        "$promotion_revision" "$api_ref" "$api_id" "$api_revision" \
        "$arena_ref" "$arena_id" "$arena_revision" \
        "$web_ref" "$web_id" "$web_revision" "$CADDY_REF" "$caddy_id" \
        "$containers" <<'PY'
import json
import sys
from pathlib import Path

(
    path,
    release_id,
    created_at,
    verified_at,
    scope,
    promotion_revision,
    api_ref,
    api_id,
    api_revision,
    arena_ref,
    arena_id,
    arena_revision,
    web_ref,
    web_id,
    web_revision,
    caddy_ref,
    caddy_id,
    container_rows,
) = sys.argv[1:]

containers = {}
for row in filter(None, container_rows.splitlines()):
    service, image_id, state = row.split("\t", 2)
    containers[service] = {"image_id": image_id, "state": state}

payload = {
    "schema": "sybil.release.v1",
    "release_id": release_id,
    "created_at": created_at,
    "verified_at": verified_at or None,
    "status": "verified" if verified_at else "pending",
    "scope": scope,
    "promotion_source_revision": promotion_revision,
    "images": {
        "sybil-api": {
            "ref": api_ref,
            "image_id": api_id,
            "source_revision": api_revision,
        },
        "sybil-arena": {
            "ref": arena_ref,
            "image_id": arena_id,
            "source_revision": arena_revision,
        },
        "sybil-web": {
            "ref": web_ref,
            "image_id": web_id,
            "source_revision": web_revision,
        },
        "caddy": {
            "ref": caddy_ref,
            "image_id": caddy_id,
            "source_revision": None,
        },
    },
    "verified_containers": containers,
}
Path(path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY
}

activate_release() {
    local server="$1" release_id="$2"
    ssh "$server" \
        "cd /opt/sybil/releases && test -f '$release_id/images.env' && ln -s '$release_id/images.env' '.current-$release_id' && mv -Tf '.current-$release_id' current.env"
}

compose_up() {
    local server="$1" scope="$2" force="${3:-0}"
    ssh "$server" bash -s -- "$scope" "$force" <<'REMOTE'
set -euo pipefail
scope=$1
force=$2
cd /opt/sybil
files=(-f docker-compose.yml -f docker-compose.prod.yml)
if grep -q '^TELEGRAM_BOT_TOKEN=.' .env && grep -q '^TELEGRAM_CHAT_ID=.' .env; then
    files+=(-f docker-compose.telegram.yml)
fi
compose=(docker compose --env-file .env --env-file releases/current.env "${files[@]}" --profile integrations --profile ops)
extra=(-d)
[[ "$force" == "0" ]] || extra+=(--force-recreate)
case "$scope" in
    api)
        "${compose[@]}" up "${extra[@]}" sybil-history sybil-api sybil-native-admin sybil-native-mm sybil-polymarket
        ;;
    arena)
        "${compose[@]}" up "${extra[@]}" sybil-arena sybil-arena-dashboard caddy
        ;;
    web)
        "${compose[@]}" up "${extra[@]}" sybil-web caddy
        ;;
    all)
        "${compose[@]}" up "${extra[@]}" --remove-orphans
        ;;
    *)
        echo "unknown release scope: $scope" >&2
        exit 2
        ;;
esac
REMOTE
}

services_for_scope() {
    case "$1" in
        api)
            printf '%s\n' sybil-api sybil-history sybil-native-admin sybil-native-mm sybil-polymarket
            ;;
        arena)
            printf '%s\n' sybil-arena sybil-arena-dashboard caddy
            ;;
        web)
            printf '%s\n' sybil-web caddy
            ;;
        all)
            printf '%s\n' sybil-api sybil-history sybil-native-admin sybil-native-mm \
                sybil-polymarket sybil-arena sybil-arena-dashboard sybil-web caddy
            ;;
    esac
}

expected_id_for_service() {
    local service="$1" api_id="$2" arena_id="$3" web_id="$4" caddy_id="$5"
    case "$service" in
        sybil-api|sybil-history|sybil-native-admin|sybil-native-mm|sybil-polymarket)
            printf '%s\n' "$api_id"
            ;;
        sybil-arena|sybil-arena-dashboard)
            printf '%s\n' "$arena_id"
            ;;
        sybil-web)
            printf '%s\n' "$web_id"
            ;;
        caddy)
            printf '%s\n' "$caddy_id"
            ;;
    esac
}

verify_containers() {
    local server="$1" scope="$2" api_id="$3" arena_id="$4" web_id="$5" caddy_id="$6"
    local service expected actual cid observed_state verified_state rows=""
    while IFS= read -r service; do
        expected="$(expected_id_for_service "$service" "$api_id" "$arena_id" "$web_id" "$caddy_id")"
        cid="$(ssh -n "$server" "docker ps -a --filter 'label=com.docker.compose.project=sybil' --filter 'label=com.docker.compose.service=$service' --format='{{.ID}}'")"
        [[ -n "$cid" && "$cid" != *$'\n'* ]] || die "expected one $service container"
        actual="$(ssh -n "$server" "docker inspect --format='{{.Image}}' '$cid'")"
        [[ "$actual" == "$expected" ]] \
            || die "$service runs $actual, expected immutable image $expected"
        observed_state="$(ssh -n "$server" "docker inspect --format='{{.State.Status}}:{{.State.ExitCode}}' '$cid'")"
        case "$service:$observed_state" in
            sybil-native-admin:exited:0)
                verified_state="exited:0"
                ;;
            *:running:*)
                verified_state="running"
                ;;
            *)
                die "$service has unacceptable lifecycle state $observed_state"
                ;;
        esac
        rows+="${service}"$'\t'"${actual}"$'\t'"${verified_state}"$'\n'
    done < <(services_for_scope "$scope")
    printf '%s' "$rows"
}

read_manifest_images() {
    local path="$1"
    python3 - "$path" <<'PY'
import json
import sys

record = json.load(open(sys.argv[1], encoding="utf-8"))
if record.get("schema") != "sybil.release.v1" or record.get("status") != "verified":
    raise SystemExit("release is not a verified sybil.release.v1 record")
for name in ("sybil-api", "sybil-arena", "sybil-web", "caddy"):
    image = record["images"][name]
    print(name, image["ref"], image["image_id"], image.get("source_revision") or "-")
PY
}

promote() {
    local scope="$1" server="$2"
    case "$scope" in api|arena|web|all) ;; *) usage ;; esac
    cd "$ROOT"
    local revision current_env api_ref arena_ref web_ref
    revision="$(source_revision)"
    api_ref="sybil-api:$revision"
    arena_ref="sybil-arena:$revision"
    web_ref="sybil-web:$revision"

    if [[ "$scope" != "all" ]]; then
        current_env="$(ssh "$server" 'cat /opt/sybil/releases/current.env' 2>/dev/null)" \
            || die "scoped promotion requires an existing immutable release; run deploy-all first"
        [[ "$scope" == "api" ]] || api_ref="$(env_ref "$current_env" SYBIL_API_IMAGE)"
        [[ "$scope" == "arena" ]] || arena_ref="$(env_ref "$current_env" SYBIL_ARENA_IMAGE)"
        [[ "$scope" == "web" ]] || web_ref="$(env_ref "$current_env" SYBIL_WEB_IMAGE)"
    fi

    case "$scope" in
        api) build_image sybil-api SYBIL_API_IMAGE "$api_ref" "$revision" ;;
        arena) build_image sybil-arena SYBIL_ARENA_IMAGE "$arena_ref" "$revision" ;;
        web) build_image sybil-web SYBIL_WEB_IMAGE "$web_ref" "$revision" ;;
        all)
            build_image sybil-api SYBIL_API_IMAGE "$api_ref" "$revision"
            build_image sybil-arena SYBIL_ARENA_IMAGE "$arena_ref" "$revision"
            build_image sybil-web SYBIL_WEB_IMAGE "$web_ref" "$revision"
            ;;
    esac

    case "$scope" in
        api) transfer_image "$server" "$api_ref" ;;
        arena) transfer_image "$server" "$arena_ref" ;;
        web) transfer_image "$server" "$web_ref" ;;
        all)
            transfer_image "$server" "$api_ref"
            transfer_image "$server" "$arena_ref"
            transfer_image "$server" "$web_ref"
            ;;
    esac
    ensure_remote_caddy "$server"

    local api_id arena_id web_id caddy_id api_revision arena_revision web_revision
    api_id="$(remote_image_id "$server" "$api_ref")"
    arena_id="$(remote_image_id "$server" "$arena_ref")"
    web_id="$(remote_image_id "$server" "$web_ref")"
    caddy_id="$(remote_image_id "$server" "$CADDY_REF")"
    for value in "$api_id" "$arena_id" "$web_id" "$caddy_id"; do
        [[ "$value" =~ ^sha256:[0-9a-f]{64}$ ]] || die "host is missing a release image"
    done
    api_revision="$(remote_image_revision "$server" "$api_ref")"
    arena_revision="$(remote_image_revision "$server" "$arena_ref")"
    web_revision="$(remote_image_revision "$server" "$web_ref")"
    for value in "$api_revision" "$arena_revision" "$web_revision"; do
        [[ "$value" =~ ^[0-9a-f]{40}$ ]] || die "host application image is missing its source revision"
    done

    TMP="$(mktemp -d "${TMPDIR:-/tmp}/sybil-release.XXXXXX")"
    local created release_id env_path pending_path record_path verified containers
    created="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    release_id="$(date -u +%Y%m%dT%H%M%SZ)-${revision:0:12}-$scope"
    valid_release_id "$release_id" || die "generated invalid release id"
    env_path="$TMP/images.env"
    pending_path="$TMP/manifest.json"
    record_path="$RECORD_DIR/$release_id.json"
    [[ ! -e "$record_path" ]] || die "release record already exists: $record_path"
    write_images_env "$env_path" "$api_ref" "$arena_ref" "$web_ref"
    write_manifest "$pending_path" "$release_id" "$created" "" "$scope" "$revision" \
        "$api_ref" "$arena_ref" "$web_ref" \
        "$api_id" "$api_revision" "$arena_id" "$arena_revision" \
        "$web_id" "$web_revision" "$caddy_id" ""

    ssh "$server" "mkdir -p '/opt/sybil/releases/$release_id'"
    scp "$env_path" "$pending_path" "$server:/opt/sybil/releases/$release_id/"
    activate_release "$server" "$release_id"
    compose_up "$server" "$scope" 0
    containers="$(verify_containers "$server" "$scope" "$api_id" "$arena_id" "$web_id" "$caddy_id")"
    verified="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    mkdir -p "$RECORD_DIR"
    write_manifest "$record_path" "$release_id" "$created" "$verified" "$scope" "$revision" \
        "$api_ref" "$arena_ref" "$web_ref" \
        "$api_id" "$api_revision" "$arena_id" "$arena_revision" \
        "$web_id" "$web_revision" "$caddy_id" "$containers"
    scp "$record_path" "$server:/opt/sybil/releases/$release_id/manifest.json"
    info "verified release $release_id"
    info "outside-host record: $record_path"
}

rollback() {
    local release_id="$1" server="$2" confirmation="$3"
    valid_release_id "$release_id" || die "invalid release id"
    [[ "$confirmation" == "CONFIRM" ]] \
        || die "rollback restarts the full stack; pass CONFIRM explicitly"
    TMP="$(mktemp -d "${TMPDIR:-/tmp}/sybil-rollback.XXXXXX")"
    scp "$server:/opt/sybil/releases/$release_id/images.env" "$TMP/images.env"
    scp "$server:/opt/sybil/releases/$release_id/manifest.json" "$TMP/manifest.json"

    local rows api_ref arena_ref web_ref api_id arena_id web_id caddy_id
    rows="$(read_manifest_images "$TMP/manifest.json")"
    api_ref="$(awk '$1 == "sybil-api" { print $2 }' <<<"$rows")"
    api_id="$(awk '$1 == "sybil-api" { print $3 }' <<<"$rows")"
    arena_ref="$(awk '$1 == "sybil-arena" { print $2 }' <<<"$rows")"
    arena_id="$(awk '$1 == "sybil-arena" { print $3 }' <<<"$rows")"
    web_ref="$(awk '$1 == "sybil-web" { print $2 }' <<<"$rows")"
    web_id="$(awk '$1 == "sybil-web" { print $3 }' <<<"$rows")"
    caddy_id="$(awk '$1 == "caddy" { print $3 }' <<<"$rows")"
    [[ "$(env_ref "$(cat "$TMP/images.env")" SYBIL_API_IMAGE)" == "$api_ref" ]]
    [[ "$(env_ref "$(cat "$TMP/images.env")" SYBIL_ARENA_IMAGE)" == "$arena_ref" ]]
    [[ "$(env_ref "$(cat "$TMP/images.env")" SYBIL_WEB_IMAGE)" == "$web_ref" ]]
    [[ "$(env_ref "$(cat "$TMP/images.env")" SYBIL_CADDY_IMAGE)" == "$CADDY_REF" ]]

    local ref expected actual
    while read -r _ ref expected _; do
        actual="$(remote_image_id "$server" "$ref")"
        [[ "$actual" == "$expected" ]] \
            || die "rollback image $ref is missing or changed ($actual != $expected)"
    done <<<"$rows"

    activate_release "$server" "$release_id"
    compose_up "$server" all 1
    local containers verified action_id action_path
    containers="$(verify_containers "$server" all "$api_id" "$arena_id" "$web_id" "$caddy_id")"
    verified="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    action_id="$(date -u +%Y%m%dT%H%M%SZ)-rollback-to-$release_id"
    action_path="$RECORD_DIR/$action_id.json"
    python3 - "$action_path" "$action_id" "$release_id" "$verified" "$containers" <<'PY'
import json
import sys
from pathlib import Path

path, action_id, release_id, verified_at, rows = sys.argv[1:]
containers = {}
for row in filter(None, rows.splitlines()):
    service, image_id, state = row.split("\t", 2)
    containers[service] = {"image_id": image_id, "state": state}
Path(path).write_text(
    json.dumps(
        {
            "schema": "sybil.rollback.v1",
            "action_id": action_id,
            "target_release_id": release_id,
            "verified_at": verified_at,
            "rebuilt_images": False,
            "verified_containers": containers,
        },
        indent=2,
    )
    + "\n",
    encoding="utf-8",
)
PY
    scp "$action_path" "$server:/opt/sybil/releases/$release_id/$action_id.json"
    info "rollback to $release_id verified without rebuilding"
    info "outside-host record: $action_path"
}

verify_current() {
    local server="$1"
    TMP="$(mktemp -d "${TMPDIR:-/tmp}/sybil-release-verify.XXXXXX")"
    local target
    target="$(ssh "$server" 'readlink /opt/sybil/releases/current.env')" \
        || die "host has no active immutable release"
    valid_image_ref "$target" || die "active release symlink is malformed"
    local release_id="${target%%/*}"
    scp "$server:/opt/sybil/releases/$release_id/manifest.json" "$TMP/manifest.json"
    local rows api_id arena_id web_id caddy_id
    rows="$(read_manifest_images "$TMP/manifest.json")"
    api_id="$(awk '$1 == "sybil-api" { print $3 }' <<<"$rows")"
    arena_id="$(awk '$1 == "sybil-arena" { print $3 }' <<<"$rows")"
    web_id="$(awk '$1 == "sybil-web" { print $3 }' <<<"$rows")"
    caddy_id="$(awk '$1 == "caddy" { print $3 }' <<<"$rows")"
    verify_containers "$server" all "$api_id" "$arena_id" "$web_id" "$caddy_id" >/dev/null
    info "stack matches $release_id"
}

main() {
    [[ $# -ge 1 ]] || usage
    local command=$1
    shift
    case "$command" in
        promote)
            [[ $# -eq 2 ]] || usage
            promote "$1" "$2"
            ;;
        rollback)
            [[ $# -eq 3 ]] || usage
            rollback "$1" "$2" "$3"
            ;;
        verify)
            [[ $# -eq 1 ]] || usage
            verify_current "$1"
            ;;
        -h|--help|help)
            usage 0
            ;;
        *)
            usage
            ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
