#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# ZK guest commitment staleness gate.
#
# The OpenVM guest (`zk/openvm-guest`) is consensus surface: its compiled
# artifact yields the `app_exe_commit` / `app_vm_commit` values that
# `contracts/src/OpenVmVerifierAdapter.sol` pins at deploy time. Those
# generated artifacts live under `zk/openvm-guest/openvm/` which is
# .gitignore'd, so nothing in the committed tree records "which source the
# pinned commitment was built from". This script closes that gap.
#
# It fingerprints the guest SOURCE tree (the inputs that determine the
# commitment) and stores the fingerprint in a committed lock file. CI runs
# `--check`, which recomputes the fingerprint and fails when the guest source
# changed but the lock file was not refreshed -- i.e. the guest was edited
# without regenerating the commitment. This script only COMPARES; it never
# rebuilds the guest or regenerates the on-chain commitment.
#
# SCOPE (SYB-213): the fingerprint covers the guest's full path-dependency
# closure, NOT just zk/openvm-guest/. The guest compiles `crates/sybil-zk` by
# path, which pulls in `crates/sybil-verifier`, which pulls in
# `crates/matching-engine` -- all by path, all consensus surface. Editing any of
# them changes the built guest and its app_exe_commit/app_vm_commit. Hashing
# only zk/openvm-guest/ was a real blind spot: the SYB-196 newtype migration
# moved the commitment (app_exe_commit 0x0094ea7a -> 0x0036273c) while this gate
# stayed green. See collect_source_files() for the enumerated closure.
#
# Usage:
#   scripts/zk-guest-fingerprint.sh            # --check (default, used by CI)
#   scripts/zk-guest-fingerprint.sh --check
#   scripts/zk-guest-fingerprint.sh --write    # refresh lock after a rebuild
#
# The `--write` path is for a human/release step AFTER regenerating the guest
# commitment (`just openvm-commit`): it snapshots the current source
# fingerprint and, when the local (gitignored) commit.json is present, records
# the freshly built commitment hashes for traceability.
# ---------------------------------------------------------------------------
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUEST_DIR="$REPO_ROOT/zk/openvm-guest"
LOCK_FILE="$GUEST_DIR/guest.commitment.lock.json"
COMMIT_JSON="$GUEST_DIR/openvm/release/sybil-openvm-guest.commit.json"

# Consensus-relevant guest source inputs, as paths relative to $REPO_ROOT.
#
# This MUST cover the guest's full path-dependency closure, because the compiled
# guest -- and therefore its commitment -- is built from all of it. The closure
# is (each arrow is a Cargo `path = ` dependency):
#
#   zk/openvm-guest       -> crates/sybil-zk
#   crates/sybil-zk       -> crates/sybil-verifier
#   crates/sybil-verifier -> crates/matching-engine
#   crates/matching-engine   (leaf; no path deps)
#
# We hardcode these roots rather than parse `cargo metadata`: the guest lives
# outside the workspace and needs the OpenVM prerelease toolchain to resolve, so
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
CLOSURE_CRATES=(crates/sybil-zk crates/sybil-verifier crates/matching-engine crates/sybil-l1-protocol)

collect_source_files() {
    {
        # Guest crate: manifest, lock, openvm config, and Rust sources. Listed
        # explicitly so we never sweep in openvm/ artifacts or the lock file.
        printf '%s\n' \
            "zk/openvm-guest/Cargo.toml" \
            "zk/openvm-guest/Cargo.lock" \
            "zk/openvm-guest/openvm.toml"
        (cd "$REPO_ROOT" && find zk/openvm-guest/src -type f -name '*.rs')
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
    local source_hash exe_commit vm_commit
    source_hash="$(compute_source_hash)"
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
    cat > "$LOCK_FILE" <<EOF
{
  "_comment": "Staleness pin for the OpenVM guest commitment. Regenerate with 'scripts/zk-guest-fingerprint.sh --write' AFTER rebuilding the guest commitment ('just openvm-commit'). CI runs '--check' and fails if source_sha256 no longer matches the guest source tree.",
  "openvm_tag": "v2.0.0-beta.2",
  "source_sha256": "$source_hash",
  "app_exe_commit": "$exe_commit",
  "app_vm_commit": "$vm_commit"
}
EOF
    echo "Wrote $LOCK_FILE"
    echo "  source_sha256=$source_hash"
    echo "  app_exe_commit=$exe_commit"
    echo "  app_vm_commit=$vm_commit"
}

check_lock() {
    if [ ! -f "$LOCK_FILE" ]; then
        echo "ERROR: guest commitment lock file missing: $LOCK_FILE" >&2
        echo "       Run: scripts/zk-guest-fingerprint.sh --write" >&2
        exit 1
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
    echo "OK: guest source matches pinned commitment fingerprint ($actual)."
    cross_check_commit_json
}

case "${1:---check}" in
    --check) check_lock ;;
    --write) write_lock ;;
    *)
        echo "usage: $0 [--check|--write]" >&2
        exit 2
        ;;
esac
