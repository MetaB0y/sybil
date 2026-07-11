#!/usr/bin/env python3
"""Coordinate desired OpenVM commitments with recorded deployment evidence."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
PINS = ROOT / "deploy/validity-pins.json"
SOURCES = {
    "state_transition": ROOT
    / "zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json",
    "escape_claim": ROOT
    / "zk/openvm-escape-guest/openvm/release/sybil-openvm-escape-guest.commit.json",
}


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def source_pins() -> dict[str, dict[str, str]]:
    return {name: load(path) for name, path in SOURCES.items()}


def write_desired() -> int:
    document = load(PINS)
    document["desired"] = source_pins()
    # A changed desired commitment is not deployed merely because its source
    # artifact was refreshed. Preserve old evidence, but require a new repin.
    document["status"] = "pending_redeploy"
    PINS.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(f"updated desired validity pins in {PINS.relative_to(ROOT)}")
    return 0


def check() -> int:
    document = load(PINS)
    errors: list[str] = []
    expected = source_pins()
    if document.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if document.get("desired") != expected:
        errors.append("desired pins differ from committed guest artifacts")

    status = document.get("status")
    deployed = document.get("deployed", {})
    if status not in {"pending_redeploy", "deployed"}:
        errors.append("status must be pending_redeploy or deployed")
    if status == "deployed":
        for field in (
            "settlement_address",
            "vault_address",
            "transition_adapter_address",
            "escape_adapter_address",
            "verified_at",
        ):
            if not deployed.get(field):
                errors.append(f"deployed.{field} is required when status=deployed")
        for guest in expected:
            if deployed.get(guest) != expected[guest]:
                errors.append(
                    f"deployed.{guest} must match desired pins when status=deployed"
                )

    if errors:
        print("Validity pin check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "Run `just validity-pins-write` after rebuilding guests, then record "
            "verified deployment evidence separately.",
            file=sys.stderr,
        )
        return 1

    print(f"validity pins match guest artifacts; deployment status: {status}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write-desired", action="store_true")
    args = parser.parse_args()
    return write_desired() if args.write_desired else check()


if __name__ == "__main__":
    sys.exit(main())
