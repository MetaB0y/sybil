#!/usr/bin/env python3
"""Check small, high-value documentation inventories against repository truth."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    errors: list[str] = []
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    members = [
        Path(member)
        for member in workspace["workspace"]["members"]
        if member.startswith("crates/")
    ]

    root_map = (ROOT / "AGENTS.md").read_text(encoding="utf-8")
    dependency_map = (ROOT / "docs/architecture/Crate Dependency Map.md").read_text(
        encoding="utf-8"
    )

    for member in members:
        name = member.name
        if not (ROOT / member / "AGENTS.md").is_file():
            errors.append(f"{member}/AGENTS.md is missing")
        if not re.search(rf"(?:├──|└──)\s+{re.escape(name)}/", root_map):
            errors.append(f"{name} is missing from the root repo map")
        if f"`{name}`" not in dependency_map:
            errors.append(f"{name} is missing from Crate Dependency Map.md")

    for path in sorted((ROOT / "design").glob("*.md")):
        text = path.read_text(encoding="utf-8")
        frontmatter = re.match(r"^---\n(.*?)\n---\n", text, re.DOTALL)
        if frontmatter is None or not re.search(
            r"^status:\s*\S+", frontmatter.group(1), re.MULTILINE
        ):
            errors.append(f"{path.relative_to(ROOT)} needs frontmatter with status")

    if errors:
        print("Documentation sync errors:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(
        f"doc sync clean: {len(members)} workspace crates and "
        f"{len(list((ROOT / 'design').glob('*.md')))} top-level design files"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
