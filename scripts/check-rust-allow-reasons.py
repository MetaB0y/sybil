#!/usr/bin/env python3
"""Reject unexplained authored Rust lint suppressions.

Clippy's allow-attributes-without-reason lint cannot exclude path dependencies:
command-line lint flags also reach the commitment-fingerprinted guest closure.
This source gate provides the intended policy while preserving the exact source
used by the currently pinned OpenVM commitments.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", ".jj", ".venv", "dist", "node_modules", "target"}

# These attributes predate the current guest commitments. Every site has an
# adjacent explanatory comment; changing even attribute metadata correctly
# invalidates the source fingerprint and therefore waits for a real rebuild.
PINNED_GUEST_EXCEPTIONS = Counter(
    {
        ("crates/matching-engine/src/order.rs", "clippy::disallowed_types"): 1,
        ("crates/matching-engine/src/order_builder.rs", "clippy::too_many_arguments"): 2,
        ("crates/matching-engine/src/types.rs", "clippy::disallowed_types"): 2,
        ("crates/sybil-verifier/src/account_keys.rs", "clippy::too_many_arguments"): 1,
        (
            "crates/sybil-zk/src/guest_commitments.rs",
            "clippy::needless_borrows_for_generic_args",
        ): 1,
        ("zk/openvm-escape-guest/src/main.rs", "unused_imports"): 1,
        ("zk/openvm-guest/src/main.rs", "unused_imports"): 1,
    }
)

ALLOW_ATTRIBUTE = re.compile(
    r"#\s*\[\s*allow\s*\((?P<body>.*?)\)\s*\]",
    flags=re.DOTALL,
)


def rust_sources() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("*.rs")
        if not any(part in SKIP_DIRS for part in path.relative_to(ROOT).parts)
    )


def normalized_lints(body: str) -> str:
    without_comments = re.sub(r"//.*?$|/\*.*?\*/", "", body, flags=re.MULTILINE | re.DOTALL)
    return ",".join(
        part.strip()
        for part in without_comments.split(",")
        if part.strip() and not part.lstrip().startswith("reason")
    )


def main() -> int:
    observed_exceptions: Counter[tuple[str, str]] = Counter()
    unexplained: list[str] = []

    for path in rust_sources():
        relative = path.relative_to(ROOT).as_posix()
        text = path.read_text()
        for match in ALLOW_ATTRIBUTE.finditer(text):
            body = match.group("body")
            if re.search(r"\breason\s*=", body):
                continue
            key = (relative, normalized_lints(body))
            line = text.count("\n", 0, match.start()) + 1
            if key in PINNED_GUEST_EXCEPTIONS:
                observed_exceptions[key] += 1
            else:
                unexplained.append(f"{relative}:{line}: #[allow({key[1]})] has no reason")

    missing = PINNED_GUEST_EXCEPTIONS - observed_exceptions
    excess = observed_exceptions - PINNED_GUEST_EXCEPTIONS
    if unexplained or missing or excess:
        for finding in unexplained:
            print(finding, file=sys.stderr)
        for (path, lints), count in sorted(missing.items()):
            print(
                f"{path}: expected {count} pinned #[allow({lints})] exception(s) not found",
                file=sys.stderr,
            )
        for (path, lints), count in sorted(excess.items()):
            print(
                f"{path}: found {count} unlisted pinned #[allow({lints})] exception(s)",
                file=sys.stderr,
            )
        return 1

    total = sum(observed_exceptions.values())
    print(f"Rust allow reasons: all authored suppressions explained; {total} pinned exceptions")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
