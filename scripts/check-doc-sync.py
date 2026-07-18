#!/usr/bin/env python3
"""Check small, high-value documentation inventories against repository truth."""

from __future__ import annotations

import re
import sys
import tomllib
import urllib.parse
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
ADDITIONAL_INSTRUCTION_ROOTS = (
    Path("arena"),
    Path("benchmarks/solver"),
    Path("contracts"),
    Path("deploy"),
    Path("frontend"),
    Path("fuzz"),
    Path("zk"),
)
EXCLUDED_AGENT_PARTS = {".git", ".jj", ".next", ".venv", "node_modules", "target"}
WIKI_LINK_RE = re.compile(r"\[\[([^\]]+)\]\]")
MARKDOWN_LINK_RE = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")


def agent_files() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("AGENTS.md")
        if not EXCLUDED_AGENT_PARTS.intersection(path.relative_to(ROOT).parts)
    )


def check_agent_links(paths: list[Path]) -> list[str]:
    errors: list[str] = []
    note_names = {
        path.stem for path in (ROOT / "docs/architecture").rglob("*.md")
    }
    for path in paths:
        relative = path.relative_to(ROOT)
        text = path.read_text(encoding="utf-8")
        for raw_target in WIKI_LINK_RE.findall(text):
            target = raw_target.split("|", 1)[0].split("#", 1)[0].strip()
            if target and target not in note_names:
                errors.append(f"{relative}: unknown architecture note [[{target}]]")

        for raw_target in MARKDOWN_LINK_RE.findall(text):
            target = raw_target.strip()
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            else:
                target = target.split(maxsplit=1)[0]
            parsed = urllib.parse.urlsplit(target)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            candidate = (path.parent / urllib.parse.unquote(parsed.path)).resolve()
            try:
                candidate.relative_to(ROOT)
            except ValueError:
                errors.append(f"{relative}: link escapes repository: {raw_target}")
                continue
            if not candidate.exists():
                errors.append(f"{relative}: missing linked path: {raw_target}")
    return errors


def main() -> int:
    errors: list[str] = []
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    members = [Path(member) for member in workspace["workspace"]["members"]]
    dependency_map = (ROOT / "docs/architecture/Crate Dependency Map.md").read_text(
        encoding="utf-8"
    )

    instruction_roots = [*members, *ADDITIONAL_INSTRUCTION_ROOTS]
    for member in instruction_roots:
        if not (ROOT / member / "AGENTS.md").is_file():
            errors.append(f"{member}/AGENTS.md is missing")
    instructions = agent_files()
    errors.extend(check_agent_links(instructions))

    for member in members:
        if not member.parts or member.parts[0] != "crates":
            continue
        name = member.name
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
        f"doc sync clean: {len(instruction_roots)} instruction roots, "
        f"{len(instructions)} instruction files, {len(members)} workspace crates, and "
        f"{len(list((ROOT / 'design').glob('*.md')))} top-level design files"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
