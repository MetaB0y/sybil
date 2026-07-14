#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
compose_file="$root/docker-compose.prover-soak.yml"
compose_bin=${COMPOSE_BIN:-docker-compose}
project="sybil-prover-soak-$$"
export PROVER_SOAK_DATA_ROOT="$root/target/$project"
mkdir -p "$PROVER_SOAK_DATA_ROOT"/{sequencer,prover,artifacts}

compose() {
  "$compose_bin" -p "$project" -f "$compose_file" "$@"
}

cleanup() {
  compose down --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

status_json() {
  compose exec -T sybil-prover \
    curl -fsS -H 'Authorization: Bearer prover-soak-admin' \
    http://127.0.0.1:3002/v1/status
}

wait_ready() {
  local deadline=$((SECONDS + 90))
  until compose exec -T sybil-prover \
    curl -fsS http://127.0.0.1:3002/readyz >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      compose ps
      compose logs --tail=100 sybil-api sybil-prover
      echo "timed out waiting for prover readiness" >&2
      return 1
    fi
    sleep 1
  done
}

wait_frontier() {
  local target=$1
  local deadline=$((SECONDS + 90))
  local status
  while (( SECONDS < deadline )); do
    status=$(status_json 2>/dev/null || true)
    if [[ -n "$status" ]] &&
      jq -e --argjson target "$target" \
        '.policy.proven_frontier != null and .policy.proven_frontier >= $target' \
        >/dev/null <<<"$status"; then
      printf '%s\n' "$status"
      return 0
    fi
    sleep 1
  done
  compose ps
  compose logs --tail=100 sybil-api sybil-prover
  echo "timed out waiting for proven frontier $target" >&2
  return 1
}

compose up -d
wait_ready
before=$(wait_frontier 8)
before_frontier=$(jq -r '.policy.proven_frontier' <<<"$before")

# The sequencer keeps producing and durably queuing jobs while the prover is
# hard-killed. Restart must resume from its own redb without gaps or duplicates.
compose kill -s SIGKILL sybil-prover
sleep 2
compose up -d sybil-prover
wait_ready
target=$((before_frontier + 16))
after=$(wait_frontier "$target")
epochs=$(compose exec -T sybil-prover \
  curl -fsS -H 'Authorization: Bearer prover-soak-admin' \
  http://127.0.0.1:3002/v1/epochs)

jq -e '
  [.epochs[] | select(.state.state == "proven")] as $proven |
  ($proven | length) >= 6 and
  ([range(1; $proven | length) |
    select(
      $proven[.].first_block_height !=
      ($proven[. - 1].last_block_height + 1))] |
    length) == 0
' >/dev/null <<<"$epochs"

after_frontier=$(jq -r '.policy.proven_frontier' <<<"$after")
ingested_frontier=$(jq -r '.policy.ingested_frontier' <<<"$after")
epoch_count=$(jq '[.epochs[] | select(.state.state == "proven")] | length' <<<"$epochs")
printf 'prover_compose_soak=ok before_frontier=%s after_frontier=%s ingested_frontier=%s proven_epochs=%s\n' \
  "$before_frontier" "$after_frontier" "$ingested_frontier" "$epoch_count"
