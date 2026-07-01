#!/usr/bin/env bash
# Validates the Sybil architecture Obsidian vault.
# Exit 1 on errors (broken links, missing frontmatter). Exit 0 on warnings-only or clean.

set -euo pipefail

VAULT_DIR="${1:-docs/architecture}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VAULT_PATH="$REPO_ROOT/$VAULT_DIR"

if [ ! -d "$VAULT_PATH" ]; then
    echo "ERROR: Vault directory not found: $VAULT_PATH"
    exit 1
fi

ERRORS=0
WARNINGS=0

error() { echo "  ERROR: $1"; ERRORS=$((ERRORS + 1)); }
warn()  { echo "  WARN:  $1"; WARNINGS=$((WARNINGS + 1)); }

VALID_LAYERS="core solver sequencer api oracle verification arena"
VALID_STATUSES="current planned deprecated"

# Collect all note names (without .md extension). Use indexed arrays instead
# of associative arrays so the script runs on macOS' bundled Bash 3.
NOTE_NAMES=()
for f in "$VAULT_PATH"/*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .md)"
    NOTE_NAMES+=("$name")
done

# Track incoming links for orphan detection
INCOMING_NAMES=()
INCOMING_COUNTS=()

note_exists() {
    local needle="$1"
    local note
    for note in "${NOTE_NAMES[@]}"; do
        if [ "$note" = "$needle" ]; then
            return 0
        fi
    done
    return 1
}

incoming_count() {
    local needle="$1"
    local i
    for ((i = 0; i < ${#INCOMING_NAMES[@]}; i++)); do
        if [ "${INCOMING_NAMES[$i]}" = "$needle" ]; then
            echo "${INCOMING_COUNTS[$i]}"
            return
        fi
    done
    echo 0
}

increment_incoming() {
    local needle="$1"
    local i
    for ((i = 0; i < ${#INCOMING_NAMES[@]}; i++)); do
        if [ "${INCOMING_NAMES[$i]}" = "$needle" ]; then
            INCOMING_COUNTS[$i]=$((INCOMING_COUNTS[$i] + 1))
            return
        fi
    done
    INCOMING_NAMES+=("$needle")
    INCOMING_COUNTS+=(1)
}

frontmatter_field() {
    local field="$1"
    awk -F: -v field="$field" '
        $1 == field {
            sub(/^[^:]*:[[:space:]]*/, "")
            print
            exit
        }
    '
}

date_to_epoch() {
    local value="$1"
    if date -d "$value" +%s >/dev/null 2>&1; then
        date -d "$value" +%s
    elif date -j -f "%Y-%m-%d" "$value" +%s >/dev/null 2>&1; then
        date -j -f "%Y-%m-%d" "$value" +%s
    else
        echo 0
    fi
}

echo "=== Vault Validation: $VAULT_PATH ==="
echo ""

# ── Check each note ──────────────────────────────────────────────────────────

for f in "$VAULT_PATH"/*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .md)"
    echo "[$name]"

    content="$(cat "$f")"

    # ── 1. Wiki-links resolve ────────────────────────────────────────────────
    while IFS= read -r link; do
        # Strip anchor (e.g., [[Target#section]] → Target)
        target="${link%%|*}"
        target="${target%%#*}"
        if ! note_exists "$target"; then
            error "Broken wiki-link [[$link]] → '$target.md' not found"
        else
            # Track incoming link
            increment_incoming "$target"
        fi
    done < <(awk '
        {
            line = $0
            while (match(line, /\[\[[^]]+\]\]/)) {
                print substr(line, RSTART + 2, RLENGTH - 4)
                line = substr(line, RSTART + RLENGTH)
            }
        }
    ' "$f")

    # ── 2. Frontmatter schema ────────────────────────────────────────────────
    if ! head -1 "$f" | grep -q '^---$'; then
        error "Missing frontmatter"
        continue
    fi

    # Extract frontmatter (between first and second ---)
    frontmatter="$(awk '/^---$/{n++; next} n==1{print} n>=2{exit}' "$f")"

    # Check required fields
    tags="$(printf '%s\n' "$frontmatter" | frontmatter_field tags)"
    layer="$(printf '%s\n' "$frontmatter" | frontmatter_field layer)"
    status="$(printf '%s\n' "$frontmatter" | frontmatter_field status)"
    last_verified="$(printf '%s\n' "$frontmatter" | frontmatter_field last_verified)"

    if [ -z "$tags" ]; then
        error "Missing 'tags' in frontmatter"
    fi
    if [ -z "$layer" ]; then
        error "Missing 'layer' in frontmatter"
    elif ! echo "$VALID_LAYERS" | grep -qw "$layer"; then
        error "Invalid layer '$layer' (valid: $VALID_LAYERS)"
    fi
    if [ -z "$status" ]; then
        error "Missing 'status' in frontmatter"
    elif ! echo "$VALID_STATUSES" | grep -qw "$status"; then
        error "Invalid status '$status' (valid: $VALID_STATUSES)"
    fi
    if [ -z "$last_verified" ]; then
        error "Missing 'last_verified' in frontmatter"
    elif ! echo "$last_verified" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
        error "Invalid last_verified '$last_verified' (expected YYYY-MM-DD)"
    else
        # ── 3. Staleness check ───────────────────────────────────────────────
        verified_epoch="$(date_to_epoch "$last_verified")"
        now_epoch="$(date +%s)"
        days_old=$(( (now_epoch - verified_epoch) / 86400 ))
        if [ "$days_old" -gt 90 ]; then
            warn "Stale — last_verified $last_verified ($days_old days ago)"
        fi
    fi

    # ── 6. Code path references ──────────────────────────────────────────────
    while IFS= read -r path; do
        # Clean up path (trim whitespace, backticks)
        path="$(echo "$path" | sed 's/`//g; s/^[[:space:]]*//; s/[[:space:]]*$//')"
        [ -z "$path" ] && continue
        # Only check paths that look like crate paths
        if [[ "$path" == crates/* || "$path" == arena/* || "$path" == viz/* || "$path" == design/* ]]; then
            if [ ! -e "$REPO_ROOT/$path" ]; then
                warn "Code path not found: $path"
            fi
        fi
    done < <(awk '
        {
            line = $0
            while (match(line, />[[:space:]]*`[^`]+`/)) {
                path = substr(line, RSTART, RLENGTH)
                sub(/^>[[:space:]]*`/, "", path)
                sub(/`$/, "", path)
                print path
                line = substr(line, RSTART + RLENGTH)
            }
        }
    ' "$f")

done

echo ""

# ── 4 & 5. Orphan and weakly connected notes ────────────────────────────────

echo "=== Link Analysis ==="
for name in "${NOTE_NAMES[@]}"; do
    incoming="$(incoming_count "$name")"

    # Skip the MOC (Sybil Architecture) for orphan check
    if [ "$name" = "Sybil Architecture" ]; then
        continue
    fi

    if [ "$incoming" -eq 0 ]; then
        warn "Orphan note: '$name' has zero incoming wiki-links"
    elif [ "$incoming" -le 1 ]; then
        warn "Weakly connected: '$name' has only $incoming incoming wiki-link(s)"
    fi
done

echo ""
echo "=== Summary ==="
echo "  Notes:    ${#NOTE_NAMES[@]}"
echo "  Errors:   $ERRORS"
echo "  Warnings: $WARNINGS"

if [ "$ERRORS" -gt 0 ]; then
    echo ""
    echo "FAILED — fix errors above"
    exit 1
fi

if [ "$WARNINGS" -gt 0 ]; then
    echo ""
    echo "PASSED with warnings"
    exit 0
fi

echo ""
echo "PASSED — vault is clean"
exit 0
