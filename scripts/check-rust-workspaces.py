#!/usr/bin/env python3
"""Check Rust version, edition, and workspace metadata stay coordinated."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
STANDALONE = (
    Path("fuzz/Cargo.toml"),
    Path("zk/openvm-guest/Cargo.toml"),
    Path("zk/openvm-escape-guest/Cargo.toml"),
    Path("zk/openvm-tools/Cargo.toml"),
)
OPENVM_MSRV = "1.94"


def load(path: Path) -> dict:
    with (ROOT / path).open("rb") as handle:
        return tomllib.load(handle)


def main() -> int:
    errors: list[str] = []
    toolchain = load(Path("rust-toolchain.toml"))["toolchain"]["channel"]
    match = re.fullmatch(r"(\d+\.\d+)(?:\.0)?", toolchain)
    if match is None:
        errors.append(f"rust-toolchain.toml has unsupported channel {toolchain!r}")
        host_version = toolchain
    else:
        host_version = match.group(1)

    root = load(Path("Cargo.toml"))
    workspace_package = root["workspace"]["package"]
    for key, expected in (
        ("version", "0.1.0"),
        ("edition", "2024"),
        ("rust-version", OPENVM_MSRV),
    ):
        if workspace_package.get(key) != expected:
            errors.append(
                f"Cargo.toml workspace.package.{key} must be {expected!r}"
            )

    for member in root["workspace"]["members"]:
        manifest = Path(member) / "Cargo.toml"
        package = load(manifest)["package"]
        for key in ("version", "edition", "rust-version"):
            if package.get(key) != {"workspace": True}:
                errors.append(f"{manifest}: package.{key} must inherit from workspace")

    for manifest in STANDALONE:
        package = load(manifest)["package"]
        if package.get("edition") != "2024":
            errors.append(f"{manifest}: package.edition must be '2024'")
        if package.get("rust-version") != OPENVM_MSRV:
            errors.append(
                f"{manifest}: package.rust-version must be {OPENVM_MSRV!r}"
            )

    dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")
    if f"FROM rust:{host_version}-" not in dockerfile:
        errors.append(f"Dockerfile must use rust:{host_version}-*")

    for workflow in sorted((ROOT / ".github/workflows").glob("*.yml")):
        text = workflow.read_text(encoding="utf-8")
        for configured in re.findall(r'toolchain:\s*["\']?([^"\'\s]+)', text):
            if configured != toolchain:
                errors.append(
                    f"{workflow.relative_to(ROOT)}: Rust toolchain {configured!r} "
                    f"must match {toolchain!r}"
                )

    if errors:
        print("Rust workspace consistency check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print(
        f"Rust workspaces agree on host {toolchain}, OpenVM-compatible MSRV "
        f"{OPENVM_MSRV}, Edition 2024, and shared metadata"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
