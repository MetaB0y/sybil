#!/usr/bin/env bash
# Restart-resilience gate (SYB-267): catch boot-OOM / cold-start-restore
# failures that a running-stack smoke cannot see (the SYB-266 class — a green
# demo box that is one reboot away from a dead API).
#
# Restarts the API container, then fails closed unless it comes back healthy
# without OOM kills or a boot loop. Opt-in (~20s API downtime): run before
# demos and after memory/config changes, NOT on every deploy.
#
# Usage:
#   scripts/restart-resilience-check.sh [--container NAME] [--ssh HOST]
#                                       [--timeout SECONDS]
#   just deploy-verify-restart          # against the live devnet over SSH
#
# Environment:
#   SYBIL_SMOKE_DOCKER_SSH  optional SSH target (same convention as the smoke)

set -uo pipefail

CONTAINER="sybil-sybil-api-1"
SSH_TARGET="${SYBIL_SMOKE_DOCKER_SSH:-}"
TIMEOUT=120

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage 0 ;;
        --container) CONTAINER="${2:?missing name after --container}"; shift 2 ;;
        --ssh) SSH_TARGET="${2:?missing host after --ssh}"; shift 2 ;;
        --timeout) TIMEOUT="${2:?missing seconds after --timeout}"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; usage 2 ;;
    esac
done

dk() {
    if [[ -n "$SSH_TARGET" ]]; then
        # %q-quote every argument so Go-template formats with spaces survive
        # the remote shell.
        # shellcheck disable=SC2029
        ssh -o BatchMode=yes "$SSH_TARGET" "docker $(printf '%q ' "$@")"
    else
        docker "$@"
    fi
}

inspect() { dk inspect "$CONTAINER" --format "$1" 2>/dev/null; }

fail() {
    echo "FAIL: $1" >&2
    echo "--- last 40 log lines from $CONTAINER ---" >&2
    dk logs --tail 40 "$CONTAINER" >&2 2>&1 || true
    exit 1
}

STATE="$(inspect '{{.State.Status}}')" || true
[[ -n "$STATE" ]] || fail "container '$CONTAINER' not found"

RESTARTS_BEFORE="$(inspect '{{.RestartCount}}')"
echo "restart-resilience: restarting $CONTAINER (restart_count=$RESTARTS_BEFORE, timeout=${TIMEOUT}s)..."

T0=$(date +%s)
dk restart "$CONTAINER" >/dev/null || fail "docker restart failed"

HEALTHY_AT=""
while :; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - T0))
    HEALTH="$(inspect '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}')"
    OOM="$(inspect '{{.State.OOMKilled}}')"
    RESTARTS_NOW="$(inspect '{{.RestartCount}}')"

    [[ "$OOM" == "true" ]] && fail "OOMKilled=true after ${ELAPSED}s — cold-start restore peak exceeds the memory cap (SYB-266 class)"
    if [[ -n "$RESTARTS_NOW" && -n "$RESTARTS_BEFORE" && "$RESTARTS_NOW" -gt "$RESTARTS_BEFORE" ]]; then
        fail "RestartCount climbed ${RESTARTS_BEFORE} -> ${RESTARTS_NOW} after ${ELAPSED}s — boot loop"
    fi
    if [[ "$HEALTH" == "healthy" ]]; then HEALTHY_AT=$ELAPSED; break; fi
    if [[ "$HEALTH" == "none" && "$(inspect '{{.State.Status}}')" == "running" && $ELAPSED -ge 10 ]]; then
        # No healthcheck defined: treat 10s of stable running as up.
        HEALTHY_AT=$ELAPSED; break
    fi
    [[ $ELAPSED -ge $TIMEOUT ]] && fail "not healthy after ${TIMEOUT}s (health=$HEALTH)"
    sleep 3
done

RSS="$(dk stats --no-stream --format '{{.MemUsage}}' "$CONTAINER" 2>/dev/null | head -1)"
echo "OK: $CONTAINER healthy ${HEALTHY_AT}s after restart; restart_count stable at ${RESTARTS_BEFORE}; settled mem ${RSS:-unknown}"
echo "NOTE: restore peak grows with chain length — watch the margin against the container mem_limit over time."
