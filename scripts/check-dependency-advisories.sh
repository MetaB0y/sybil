#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for tool in cargo uv pnpm; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "required dependency-audit tool is unavailable: $tool" >&2
        exit 1
    fi
done

# Fuzz is an isolated Cargo workspace but compiles the production verifier and
# sequencer sources. Its lock must therefore use the same Commonware API graph
# as the root validity workspace. Keep this assertion here because changing the
# compatible source requirements would itself change the pinned guest closure.
python3 - <<'PY'
from pathlib import Path
import tomllib

expected = "2026.5.0"
for lock_path in (Path("Cargo.lock"), Path("fuzz/Cargo.lock")):
    packages = tomllib.loads(lock_path.read_text())["package"]
    mismatches = sorted(
        f"{package['name']} {package['version']}"
        for package in packages
        if package["name"].startswith("commonware-")
        and package["version"] != expected
    )
    if mismatches:
        raise SystemExit(
            f"{lock_path}: Commonware must match the pinned validity graph "
            f"{expected}; found {', '.join(mismatches)}"
        )
PY

if ! cargo audit --version >/dev/null 2>&1; then
    echo "cargo-audit 0.22.2 is required (cargo install cargo-audit --version 0.22.2 --locked)" >&2
    exit 1
fi

# RUSTSEC-2020-0071 is reachable only through scip-sys's unused direct zip
# build dependency in the feature-gated research MILP solver. scip-sys extracts
# with zip-extract/zip 0.6 and never calls zip 0.5's vulnerable time functions.
# RUSTSEC-2025-0055 is tracing-subscriber 0.2.25 through ark-relations in the
# pinned Commonware 2026.5 validity graph. Sybil does not initialize the R1CS
# tracing layer or format its fields; upgrading Commonware requires a scheduled
# guest rebuild, so the consensus fingerprint takes precedence here.
# RUSTSEC-2026-0002 is confined to num-prime in OpenVM v2 host-side proc-macro
# expansion; no Sybil or OpenVM path calls lru::IterMut. Both upstream
# All three upstream eliminations remain tracked rather than silently accepted.
rust_ignores=(
    --ignore RUSTSEC-2020-0071
    --ignore RUSTSEC-2025-0055
    --ignore RUSTSEC-2026-0002
)

cargo audit --file Cargo.lock --deny unsound "${rust_ignores[@]}"
for lock in \
    fuzz/Cargo.lock \
    zk/openvm-guest/Cargo.lock \
    zk/openvm-escape-guest/Cargo.lock \
    zk/openvm-tools/Cargo.lock
do
    cargo audit --file "$lock" --no-fetch --deny unsound "${rust_ignores[@]}"
done

(
    cd frontend/web
    pnpm audit --audit-level=moderate
)

audit_uv_project() {
    local project="$1"
    local requirements
    requirements="$(mktemp)"
    trap 'rm -f "$requirements"' RETURN

    (
        cd "$project"
        uv export --frozen --no-dev --format requirements-txt
    ) >"$requirements"
    # Local projects are trusted authored code, not a registry dependency, and
    # cannot be audited as a hash-pinned editable requirement.
    sed -i '/^-e \.$/d' "$requirements"
    uvx --from pip-audit==2.10.1 pip-audit --disable-pip -r "$requirements"
}

audit_uv_project arena
audit_uv_project viz
