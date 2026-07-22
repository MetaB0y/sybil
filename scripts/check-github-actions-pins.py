#!/usr/bin/env python3
"""Reject mutable third-party references in GitHub Actions workflows."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"
USES_LINE = re.compile(
    r"^\s*(?:-\s*)?uses:\s*(?P<reference>[^\s#]+)"
    r"(?:\s+#\s*(?P<label>\S.*))?\s*$"
)
COMMIT = re.compile(r"[0-9a-f]{40}")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def validate(path: Path) -> tuple[int, list[str]]:
    count = 0
    failures: list[str] = []
    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        if "uses:" not in line:
            continue
        match = USES_LINE.match(line)
        if match is None:
            failures.append(f"{path.relative_to(ROOT)}:{line_number}: unparseable uses entry")
            continue
        reference = match.group("reference").strip("'\"")
        label = match.group("label")
        if reference.startswith("./"):
            continue
        count += 1
        if reference.startswith("docker://"):
            pin = reference.rpartition("@")[2]
            if DIGEST.fullmatch(pin) is None:
                failures.append(
                    f"{path.relative_to(ROOT)}:{line_number}: container action must use sha256"
                )
        else:
            owner_repo, separator, pin = reference.rpartition("@")
            if not separator or "/" not in owner_repo or COMMIT.fullmatch(pin) is None:
                failures.append(
                    f"{path.relative_to(ROOT)}:{line_number}: action must use a full commit SHA"
                )
        if not label:
            failures.append(
                f"{path.relative_to(ROOT)}:{line_number}: pinned action needs a version comment"
            )
    return count, failures


def main() -> int:
    paths = sorted((*WORKFLOWS.glob("*.yml"), *WORKFLOWS.glob("*.yaml")))
    total = 0
    failures: list[str] = []
    for path in paths:
        count, path_failures = validate(path)
        total += count
        failures.extend(path_failures)
    if failures:
        print("GitHub Actions pin check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"GitHub Actions pins: {total} immutable external references across {len(paths)} workflows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
