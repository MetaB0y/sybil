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

# Print one row per compose container: "<name> <status> <health|none>".
smoke_compose_service_rows() {
    local docker_ssh=$1 compose_project=$2
    smoke_docker_run "$docker_ssh" \
        "docker ps -aq --filter label=com.docker.compose.project=$compose_project | xargs -r docker inspect --format '{{.Name}} {{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}'"
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

    local saw_api=0 name status health
    while read -r name status health; do
        [[ -z "$name" ]] && continue
        name="${name#/}"
        [[ "$name" == *sybil-api* ]] && saw_api=1
        if [[ "$status" == "running" && ( "$health" == "none" || "$health" == "healthy" ) ]]; then
            "$pass_cb" "service $name: $status/$health"
        else
            "$fail_cb" "service $name: $status/$health (not running-and-healthy)"
        fi
    done <<< "$rows"
    if [[ "$saw_api" -ne 1 ]]; then
        "$fail_cb" "required service sybil-api not found in project '$compose_project'"
    fi
}
