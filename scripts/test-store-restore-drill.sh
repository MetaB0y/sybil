#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SCRIPT="$ROOT/scripts/store-restore-drill.sh"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/store-restore-drill-test.XXXXXX")
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

BACKUP="$TMP/backup"
FAKE_BIN="$TMP/bin"
mkdir -p "$BACKUP/store/sybil.qmdb" "$FAKE_BIN" "$TMP/work"
printf 'redb\n' > "$BACKUP/store/sybil.redb"
printf 'qmdb\n' > "$BACKUP/store/sybil.qmdb/state"
printf '%s\n' \
    '{"schema":"sybil.store-backup.v3","source":{"retain_validity_artifacts":false},"expected":{"height":9,"committed_state_root":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","replayed_state_root":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","account_id":42,"account":{"account_id":42,"balance_nanos":7}}}' \
    > "$BACKUP/manifest.json"
(
    cd "$BACKUP/store"
    find . -type f -print0 | sort -z | xargs -0 sha256sum
) > "$BACKUP/SHA256SUMS"

printf '%s\n' \
    '#!/usr/bin/env bash' \
    'if [[ "${1:-}" == compose ]]; then exit 1; fi' \
    'if [[ "${1:-}" == ps && "${FAKE_LIVE_API:-0}" == 1 ]]; then echo live-api; fi' \
    'exit 0' \
    > "$FAKE_BIN/docker"
printf '%s\n' \
    '#!/usr/bin/env bash' \
    'printf "%s|retain=%s\n" "$*" "${SYBIL_ITEST_RETAIN_VALIDITY_ARTIFACTS:-unset}" >> "${FAKE_COMPOSE_LOG:?}"' \
    'case " $* " in' \
    '  *" up -d --no-build sybil-api "*) touch "${FAKE_UP:?}" ;;' \
    '  *" down -v --remove-orphans "*) touch "${FAKE_DOWN:?}" ;;' \
    'esac' \
    'exit 0' \
    > "$FAKE_BIN/docker-compose"
printf '%s\n' \
    '#!/usr/bin/env bash' \
    'exit 1' \
    > "$FAKE_BIN/curl"
chmod +x "$FAKE_BIN/docker" "$FAKE_BIN/docker-compose" "$FAKE_BIN/curl"

OUTPUT="$TMP/preflight-output"
if PATH="$FAKE_BIN:$PATH" FAKE_LIVE_API=1 FAKE_COMPOSE_LOG="$TMP/compose.log" \
    "$SCRIPT" "$BACKUP" --no-build >"$OUTPUT" 2>&1; then
    echo "FAIL: restore drill accepted a live sybil-data API without override" >&2
    exit 1
fi
grep -Fq 'refusing restore drill on a Docker daemon with a live sybil-data API' "$OUTPUT" \
    || { echo "FAIL: restore drill returned the wrong live-host error" >&2; exit 1; }
printf '  \033[32m✓\033[0m restore drill rejects implicit live-host resource sharing\n'

FAKE_UP="$TMP/up"
FAKE_DOWN="$TMP/down"
FAKE_COMPOSE_LOG="$TMP/compose.log"
PATH="$FAKE_BIN:$PATH" \
FAKE_LIVE_API=0 \
FAKE_UP="$FAKE_UP" \
FAKE_DOWN="$FAKE_DOWN" \
FAKE_COMPOSE_LOG="$FAKE_COMPOSE_LOG" \
TMPDIR="$TMP/work" \
    "$SCRIPT" "$BACKUP" --no-build --timeout 30 >"$TMP/hangup-output" 2>&1 &
DRILL_PID=$!

for _ in $(seq 1 50); do
    [[ -f "$FAKE_UP" ]] && break
    sleep 0.1
done
[[ -f "$FAKE_UP" ]] \
    || { echo "FAIL: mocked restore drill never reached Compose up" >&2; exit 1; }
kill -HUP "$DRILL_PID"
if wait "$DRILL_PID"; then
    echo "FAIL: hung-up restore drill exited successfully" >&2
    exit 1
fi
[[ -f "$FAKE_DOWN" ]] \
    || { echo "FAIL: restore drill did not run down -v after hangup" >&2; exit 1; }
grep -Fq -- '-f '"$ROOT"'/docker-compose.itest.yml down -v --remove-orphans' "$FAKE_COMPOSE_LOG" \
    || { echo "FAIL: cleanup did not use the standalone itest definition" >&2; exit 1; }
grep -Fq 'retain=false' "$FAKE_COMPOSE_LOG" \
    || { echo "FAIL: restore drill did not pass the manifest chain mode to Compose" >&2; exit 1; }
if grep -Fq 'docker-compose.yml' "$FAKE_COMPOSE_LOG"; then
    echo "FAIL: restore drill cleanup referenced the base Compose file" >&2
    exit 1
fi
printf '  \033[32m✓\033[0m restore drill traps hangup and cleans only its standalone project\n'
