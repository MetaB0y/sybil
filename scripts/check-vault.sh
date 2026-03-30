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

# Collect all note names (without .md extension)
declare -A NOTE_NAMES
for f in "$VAULT_PATH"/*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .md)"
    NOTE_NAMES["$name"]=1
done

# Track incoming links for orphan detection
declare -A INCOMING_LINKS

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
        if [ -z "${NOTE_NAMES[$target]+x}" ]; then
            error "Broken wiki-link [[$link]] → '$target.md' not found"
        else
            # Track incoming link
            INCOMING_LINKS["$target"]=$(( ${INCOMING_LINKS["$target"]:-0} + 1 ))
        fi
    done < <(grep -oP '\[\[\K[^\]]+' "$f" 2>/dev/null || true)

    # ── 2. Frontmatter schema ────────────────────────────────────────────────
    if ! head -1 "$f" | grep -q '^---$'; then
        error "Missing frontmatter"
        continue
    fi

    # Extract frontmatter (between first and second ---)
    frontmatter="$(awk '/^---$/{n++; next} n==1{print} n>=2{exit}' "$f")"

    # Check required fields
    tags="$(echo "$frontmatter" | grep -oP '^tags:\s*\K.*' || true)"
    layer="$(echo "$frontmatter" | grep -oP '^layer:\s*\K\S+' || true)"
    status="$(echo "$frontmatter" | grep -oP '^status:\s*\K\S+' || true)"
    last_verified="$(echo "$frontmatter" | grep -oP '^last_verified:\s*\K\S+' || true)"

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
    elif ! echo "$last_verified" | grep -qP '^\d{4}-\d{2}-\d{2}$'; then
        error "Invalid last_verified '$last_verified' (expected YYYY-MM-DD)"
    else
        # ── 3. Staleness check ───────────────────────────────────────────────
        verified_epoch="$(date -d "$last_verified" +%s 2>/dev/null || echo 0)"
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
    done < <(grep -oP '>\s*`\K[^`]+' "$f" 2>/dev/null || true)

done

echo ""

# ── 4 & 5. Orphan and weakly connected notes ────────────────────────────────

echo "=== Link Analysis ==="
for name in "${!NOTE_NAMES[@]}"; do
    incoming="${INCOMING_LINKS[$name]:-0}"

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
