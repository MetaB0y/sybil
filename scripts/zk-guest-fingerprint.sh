#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# ZK guest commitment staleness gate (state-transition + Form-L escape).
#
# Both OpenVM guests (`zk/openvm-guest` and `zk/openvm-escape-guest`) are
# consensus surface. Each compiled artifact yields an independent
# `app_exe_commit` / `app_vm_commit` pair pinned by its own verifier adapter.
# Their
# generated artifacts live under `zk/openvm-guest/openvm/` which is
# .gitignore'd, so nothing in the committed tree records "which source the
# pinned commitment was built from". This script closes that gap.
#
# It fingerprints the guest SOURCE tree and the untracked OpenVM KEY MATERIAL
# (the inputs that determine the commitment) and stores the fingerprints in a
# committed lock file. CI runs `--check`, which recomputes the fingerprints and
# fails when either input changed or is missing. This script only COMPARES; it
# never rebuilds the guest, regenerates keys, or regenerates the on-chain
# commitment.
#
# SCOPE (SYB-213): the fingerprint covers the guest's full path-dependency
# closure, NOT just zk/openvm-guest/. The guest compiles `crates/sybil-zk` by
# path, which pulls in `crates/sybil-verifier`, `crates/matching-engine`, and
# `crates/sybil-l1-protocol` -- all by path, all consensus surface. Editing any
# of them changes the built guest and its app_exe_commit/app_vm_commit. Hashing
# only zk/openvm-guest/ was a real blind spot: the SYB-196 newtype migration
# moved the commitment (app_exe_commit 0x0094ea7a -> 0x0036273c) while this gate
# stayed green. See collect_source_files() for the enumerated closure.
#
# Usage:
#   scripts/zk-guest-fingerprint.sh            # --check (default, used by CI)
#   scripts/zk-guest-fingerprint.sh --check
#   scripts/zk-guest-fingerprint.sh --write    # refresh lock after a rebuild
#
# The `--write` path is for a human/release step AFTER regenerating both guest
# commitments (`just openvm-commit-all`): it snapshots each source
# fingerprint and, when the local (gitignored) commit.json is present, records
# the freshly built commitment hashes for traceability.
# ---------------------------------------------------------------------------
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OPENVM_TAG="v2.0.0"
KEY_MATERIAL_NAMES=("app.pk" "app.vk" "agg_prefix.pk" "internal_recursive.pk")
KEY_MATERIAL_PATHS=(
    "$REPO_ROOT/zk/openvm-guest/openvm/app.pk"
    "$REPO_ROOT/zk/openvm-guest/openvm/app.vk"
    "$REPO_ROOT/zk/openvm-guest/openvm/agg_prefix.pk"
    "$HOME/.openvm/internal_recursive.pk"
)

select_guest() {
    case "$1" in
        main)
            GUEST_LABEL="main state-transition guest"
            GUEST_REL="zk/openvm-guest"
            GUEST_DIR="$REPO_ROOT/$GUEST_REL"
            LOCK_FILE="$GUEST_DIR/guest.commitment.lock.json"
            COMMIT_JSON="$GUEST_DIR/openvm/release/sybil-openvm-guest.commit.json"
            CHECK_KEY_MATERIAL=true
            CLOSURE_CRATES=(crates/sybil-zk crates/sybil-verifier crates/matching-engine crates/sybil-l1-protocol)
            ;;
        escape)
            GUEST_LABEL="Form-L escape-claim guest"
            GUEST_REL="zk/openvm-escape-guest"
            GUEST_DIR="$REPO_ROOT/$GUEST_REL"
            LOCK_FILE="$GUEST_DIR/guest.commitment.lock.json"
            COMMIT_JSON="$GUEST_DIR/openvm/release/sybil-openvm-escape-guest.commit.json"
            # Stage 2 deliberately does not run keygen. Commitments are fully
            # determined by source/config and do not consume app proving keys.
            CHECK_KEY_MATERIAL=false
            CLOSURE_CRATES=(crates/sybil-escape-claim crates/sybil-zk crates/sybil-verifier crates/matching-engine crates/sybil-l1-protocol)
            ;;
        *)
            echo "ERROR: unknown guest selector: $1" >&2
            exit 2
            ;;
    esac
}

# Consensus-relevant guest source inputs, as paths relative to $REPO_ROOT.
#
# This MUST cover the guest's full path-dependency closure, because the compiled
# guest -- and therefore its commitment -- is built from all of it. The closure
# is (each arrow is a Cargo `path = ` dependency):
#
#   zk/openvm-guest       -> crates/sybil-zk
#   crates/sybil-zk       -> crates/sybil-verifier, crates/sybil-l1-protocol
#   crates/sybil-verifier -> crates/matching-engine
#   crates/matching-engine   (leaf; no path deps)
#   crates/sybil-l1-protocol (leaf; no path deps)
#
# We hardcode these roots rather than parse `cargo metadata`: the guest lives
# outside the workspace and needs the OpenVM toolchain to resolve, so
# metadata isn't cheaply available, and shell-parsing it is fragile. Keep this
# list in sync with the guest's transitive path deps if any crate gains a new
# `path = ` dependency.
#
# Enumeration is `find`-based (no reliance on git tracking) and LC_ALL=C-sorted,
# so the hash is deterministic and identical across machines. For each closure
# crate we take every `*.rs` plus its `Cargo.toml`; `target/` build artifacts
# are excluded. The guest crate is listed explicitly (its dir also contains the
# lock file itself and the gitignored openvm/ artifacts, which must NOT be
# hashed).
#
# Over-hashing tradeoff (SAFE direction): `#[cfg(test)] mod tests` code and any
# test-only source in the closure crates get hashed here but do NOT affect the
# built guest (the guest never compiles dev-deps or tests). So a pure test edit
# can trip `--check` and demand a `--write`. That false "stale" is strictly
# preferable to a false "fresh" -- the failure mode SYB-196 exposed, where a
# real consensus drift slipped past a green gate. sybil-l1-protocol joined
# the closure when the guest gained deposit-inclusion verification (SYB-188).
collect_source_files() {
    {
        # Guest build recipe/wrapper plus crate manifest, lock, OpenVM config,
        # and Rust sources. Listed explicitly so we never sweep in openvm/
        # artifacts or the commitment lock file.
        printf '%s\n' \
            "justfile" \
            "scripts/openvm-rustc-wrapper.sh" \
            "$GUEST_REL/Cargo.toml" \
            "$GUEST_REL/Cargo.lock" \
            "$GUEST_REL/openvm.toml"
        (cd "$REPO_ROOT" && find "$GUEST_REL/src" -type f -name '*.rs')
        # Path-dep closure crates: every Rust source + the manifest.
        local crate
        for crate in "${CLOSURE_CRATES[@]}"; do
            printf '%s\n' "$crate/Cargo.toml"
            (cd "$REPO_ROOT" && find "$crate" -type f -name '*.rs' -not -path '*/target/*')
        done
    } | LC_ALL=C sort -u
}

# Deterministic fingerprint over "relpath + content" of every source file.
compute_source_hash() {
    local rel
    while IFS= read -r rel; do
        if [ ! -f "$REPO_ROOT/$rel" ]; then
            echo "ERROR: expected guest source file missing: $rel" >&2
            exit 3
        fi
        printf '%s\n' "$rel"
        sha256sum "$REPO_ROOT/$rel" | awk '{print $1}'
    done < <(collect_source_files) | sha256sum | awk '{print $1}'
}

read_lock_field() {
    # $1 = field name; prints value or empty. No jq dependency.
    local field="$1"
    [ -f "$LOCK_FILE" ] || return 0
    sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$LOCK_FILE" | head -n1
}

read_commit_json_field() {
    # $1 = field name; prints value from the committed commit.json, or empty.
    local field="$1"
    [ -f "$COMMIT_JSON" ] || return 0
    sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$COMMIT_JSON" | head -n1
}

key_material_failure() {
    local detail="$1"
    cat >&2 <<EOF
ERROR: OpenVM key material is missing or does not match the committed lock.
EOF
    printf '\n%b\n\n' "$detail" >&2
    cat >&2 <<EOF
OpenVM key material is NOT regenerable on this box — do not run setup/keygen
locally; restore the pinned files instead.
EOF
    exit 1
}

hash_key_file() {
    local name="$1" path="$2"
    if [ ! -f "$path" ]; then
        key_material_failure "Missing pinned key file: $name ($path)"
    fi
    sha256sum "$path" | awk '{print $1}'
}

check_key_material() {
    local tag name path expected actual i
    tag="$(read_lock_field openvm_tag)"
    if [ "$tag" != "$OPENVM_TAG" ]; then
        key_material_failure "OpenVM provenance mismatch: expected openvm_tag=$OPENVM_TAG, lock has openvm_tag=${tag:-<missing>}"
    fi

    for i in "${!KEY_MATERIAL_NAMES[@]}"; do
        name="${KEY_MATERIAL_NAMES[$i]}"
        path="${KEY_MATERIAL_PATHS[$i]}"
        expected="$(read_lock_field "$name")"
        if [ -z "$expected" ]; then
            key_material_failure "Lock file has no key_material SHA-256 for $name: $LOCK_FILE"
        fi
        actual="$(hash_key_file "$name" "$path")"
        if [ "$expected" != "$actual" ]; then
            key_material_failure "SHA-256 drift for $name:\n  expected: $expected\n  actual:   $actual"
        fi
    done
    echo "OK: OpenVM $OPENVM_TAG key material matches pinned SHA-256 hashes."
}

# Cross-check: the lock file's commitment hashes MUST equal the committed
# commit.json (the reviewable source of truth for the on-chain pin, committed
# via SYB-208). The lock file keeps the SOURCE-fingerprint role; commit.json
# is authoritative for the commitment hashes. If they drift, someone edited
# one without the other -- fail loudly.
cross_check_commit_json() {
    if [ ! -f "$COMMIT_JSON" ]; then
        cat >&2 <<EOF
ERROR: committed guest commitment artifact missing: $COMMIT_JSON

This JSON is committed (SYB-208) as the source of truth for the on-chain
appExeCommit/appVmCommit pin. A clean checkout always contains it; if it is
missing you likely deleted it locally -- restore it:
  git checkout -- zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json
EOF
        exit 1
    fi
    local lock_exe lock_vm json_exe json_vm
    lock_exe="$(read_lock_field app_exe_commit)"
    lock_vm="$(read_lock_field app_vm_commit)"
    json_exe="$(read_commit_json_field app_exe_commit)"
    json_vm="$(read_commit_json_field app_vm_commit)"
    if [ -z "$json_exe" ] || [ -z "$json_vm" ]; then
        echo "ERROR: committed commit.json missing app_exe_commit/app_vm_commit: $COMMIT_JSON" >&2
        exit 1
    fi
    if [ "$lock_exe" != "$json_exe" ] || [ "$lock_vm" != "$json_vm" ]; then
        cat >&2 <<EOF
ERROR: guest commitment records disagree.

  lock file (guest.commitment.lock.json):
    app_exe_commit: $lock_exe
    app_vm_commit:  $lock_vm
  committed commit.json (source of truth):
    app_exe_commit: $json_exe
    app_vm_commit:  $json_vm

commit.json is authoritative for the commitment hashes; the lock carries a
copy for the staleness workflow. Reconcile them:
  1. If commit.json is correct, refresh the lock: scripts/zk-guest-fingerprint.sh --write
  2. Refresh the deployed OpenVmVerifierAdapter pin if the commitment truly changed.
See zk/openvm-guest/README.md for the three-record model and redeploy procedure.
EOF
        exit 1
    fi
    echo "OK: lock commitment hashes match committed commit.json (exe=$json_exe)."
}

write_lock() {
    local source_hash exe_commit vm_commit app_pk app_vk agg_prefix_pk internal_recursive_pk
    source_hash="$(compute_source_hash)"
    app_pk=""
    app_vk=""
    agg_prefix_pk=""
    internal_recursive_pk=""
    if $CHECK_KEY_MATERIAL; then
        app_pk="$(hash_key_file app.pk "${KEY_MATERIAL_PATHS[0]}")"
        app_vk="$(hash_key_file app.vk "${KEY_MATERIAL_PATHS[1]}")"
        agg_prefix_pk="$(hash_key_file agg_prefix.pk "${KEY_MATERIAL_PATHS[2]}")"
        internal_recursive_pk="$(hash_key_file internal_recursive.pk "${KEY_MATERIAL_PATHS[3]}")"
    fi
    exe_commit=""
    vm_commit=""
    if [ -f "$COMMIT_JSON" ]; then
        exe_commit="$(sed -n 's/.*"app_exe_commit"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$COMMIT_JSON" | head -n1)"
        vm_commit="$(sed -n 's/.*"app_vm_commit"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$COMMIT_JSON" | head -n1)"
    else
        # Preserve previously recorded commitment hashes if the local build
        # artifact is absent (e.g. running --write without a fresh build).
        exe_commit="$(read_lock_field app_exe_commit)"
        vm_commit="$(read_lock_field app_vm_commit)"
    fi
    mkdir -p "$(dirname "$LOCK_FILE")"
    if $CHECK_KEY_MATERIAL; then
        cat > "$LOCK_FILE" <<EOF
{
  "_comment": "Staleness pin for the OpenVM guest commitment. Regenerate with 'scripts/zk-guest-fingerprint.sh --write' AFTER rebuilding the guest commitment ('just openvm-commit'). CI runs '--check' and fails if source_sha256 or pinned key material no longer matches.",
  "openvm_tag": "$OPENVM_TAG",
  "source_sha256": "$source_hash",
  "key_material": {
    "app.pk": "$app_pk",
    "app.vk": "$app_vk",
    "agg_prefix.pk": "$agg_prefix_pk",
    "internal_recursive.pk": "$internal_recursive_pk"
  },
  "app_exe_commit": "$exe_commit",
  "app_vm_commit": "$vm_commit"
}
EOF
    else
        cat > "$LOCK_FILE" <<EOF
{
  "_comment": "Staleness pin for the independent Form-L escape guest commitment. Regenerate with 'scripts/zk-guest-fingerprint.sh --write' AFTER 'just openvm-escape-commit'. No proving keys are generated or consumed by this commitment-only Stage 2 workflow.",
  "openvm_tag": "$OPENVM_TAG",
  "source_sha256": "$source_hash",
  "app_exe_commit": "$exe_commit",
  "app_vm_commit": "$vm_commit"
}
EOF
    fi
    echo "Wrote $LOCK_FILE"
    echo "  source_sha256=$source_hash"
    if $CHECK_KEY_MATERIAL; then
        echo "  key_material.app.pk=$app_pk"
        echo "  key_material.app.vk=$app_vk"
        echo "  key_material.agg_prefix.pk=$agg_prefix_pk"
        echo "  key_material.internal_recursive.pk=$internal_recursive_pk"
    fi
    echo "  app_exe_commit=$exe_commit"
    echo "  app_vm_commit=$vm_commit"
}

check_lock() {
    if [ ! -f "$LOCK_FILE" ]; then
        echo "ERROR: guest commitment lock file missing: $LOCK_FILE" >&2
        echo "       Run: scripts/zk-guest-fingerprint.sh --write" >&2
        exit 1
    fi
    if $CHECK_KEY_MATERIAL; then
        check_key_material
    fi
    local expected actual
    expected="$(read_lock_field source_sha256)"
    actual="$(compute_source_hash)"
    if [ -z "$expected" ]; then
        echo "ERROR: lock file has no source_sha256 field: $LOCK_FILE" >&2
        exit 1
    fi
    if [ "$expected" != "$actual" ]; then
        cat >&2 <<EOF
ERROR: zk/openvm-guest source changed but the pinned commitment was not regenerated.

  expected source_sha256 (committed lock): $expected
  actual   source_sha256 (working tree):   $actual

The guest is consensus surface. If you intended to change it you MUST:
  1. Rebuild the commitment:  just openvm-commit
  2. Refresh the deployed OpenVmVerifierAdapter appExeCommit/appVmCommit.
  3. Refresh this lock:        scripts/zk-guest-fingerprint.sh --write
If you did NOT intend to change guest behavior, revert the guest edit.
EOF
        exit 1
    fi
    echo "OK: $GUEST_LABEL source matches pinned commitment fingerprint ($actual)."
    cross_check_commit_json
}

MODE="${1:---check}"
case "$MODE" in
    --check|--write) ;;
    *)
        echo "usage: $0 [--check|--write]" >&2
        exit 2
        ;;
esac

for guest in main escape; do
    select_guest "$guest"
    echo "== $GUEST_LABEL =="
    case "$MODE" in
        --check) check_lock ;;
        --write) write_lock ;;
    esac
done
