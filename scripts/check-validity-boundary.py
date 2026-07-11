#!/usr/bin/env python3
"""Require an explicit deploy boundary for each validity-artifact fingerprint."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path


DEFAULT_ROOT = Path(__file__).resolve().parent.parent
DECLARATION = Path("deploy/validity-boundary.json")
FRESH_GENESIS_RUNBOOK = Path("docs/runbooks/fresh-genesis-redeploy.md")
GUESTS = {
    "state_transition": (
        Path("zk/openvm-guest/guest.commitment.lock.json"),
        Path(
            "zk/openvm-guest/openvm/release/"
            "sybil-openvm-guest.commit.json"
        ),
    ),
    "escape_claim": (
        Path("zk/openvm-escape-guest/guest.commitment.lock.json"),
        Path(
            "zk/openvm-escape-guest/openvm/release/"
            "sybil-openvm-escape-guest.commit.json"
        ),
    ),
}


class BoundaryError(ValueError):
    """A validity-boundary input or declaration is malformed."""


def load_json(root: Path, relative: Path) -> dict:
    path = root / relative
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise BoundaryError(f"required file is missing: {relative}") from error
    except json.JSONDecodeError as error:
        raise BoundaryError(f"invalid JSON in {relative}: {error}") from error
    if not isinstance(value, dict):
        raise BoundaryError(f"{relative} must contain a JSON object")
    return value


def sha256_json(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def commonware_packages(root: Path) -> list[dict[str, str]]:
    path = root / "Cargo.lock"
    try:
        with path.open("rb") as handle:
            lock = tomllib.load(handle)
    except FileNotFoundError as error:
        raise BoundaryError("required file is missing: Cargo.lock") from error
    except tomllib.TOMLDecodeError as error:
        raise BoundaryError(f"invalid TOML in Cargo.lock: {error}") from error

    packages = []
    for package in lock.get("package", []):
        name = package.get("name", "")
        if not name.startswith("commonware-"):
            continue
        packages.append(
            {
                field: package.get(field, "")
                for field in ("name", "version", "source", "checksum")
            }
        )
    if not packages:
        raise BoundaryError("Cargo.lock contains no resolved commonware packages")
    return sorted(
        packages,
        key=lambda package: (
            package["name"],
            package["version"],
            package["source"],
        ),
    )


def guest_inputs(root: Path) -> dict[str, dict[str, str]]:
    inputs = {}
    for name, (lock_path, commitment_path) in GUESTS.items():
        lock = load_json(root, lock_path)
        commitment = load_json(root, commitment_path)
        selected = {
            "openvm_tag": lock.get("openvm_tag", ""),
            "source_sha256": lock.get("source_sha256", ""),
            "app_exe_commit": commitment.get("app_exe_commit", ""),
            "app_vm_commit": commitment.get("app_vm_commit", ""),
        }
        missing = [field for field, value in selected.items() if not value]
        if missing:
            raise BoundaryError(
                f"{name} guest records are missing: {', '.join(missing)}"
            )
        for field in ("app_exe_commit", "app_vm_commit"):
            if lock.get(field) != selected[field]:
                raise BoundaryError(
                    f"{name} {field} differs between its lock and commitment JSON"
                )
        inputs[name] = selected
    return inputs


def collect_inputs(root: Path) -> dict:
    golden = load_json(root, Path("golden/golden-vectors.json"))
    golden.pop("_comment", None)
    pins = load_json(root, Path("deploy/validity-pins.json"))
    desired = pins.get("desired")
    if not isinstance(desired, dict) or not desired:
        raise BoundaryError("deploy/validity-pins.json must contain desired pins")
    return {
        "commonware_packages": commonware_packages(root),
        "golden_vectors_sha256": sha256_json(golden),
        "guests": guest_inputs(root),
        # Deployment addresses/status are evidence, not validity inputs. Only a
        # desired-pin move creates a new deployment boundary.
        "desired_validity_pins": desired,
    }


def fingerprint(inputs: dict) -> str:
    return f"sha256:{sha256_json(inputs)}"


def validate_reference(root: Path, action: str, reference: object) -> str | None:
    if not isinstance(reference, str) or not reference:
        return "decision.reference must name a repository Markdown document"
    relative = Path(reference)
    if relative.is_absolute() or ".." in relative.parts or relative.suffix != ".md":
        return "decision.reference must be a repository-relative Markdown path"
    if not relative.parts or relative.parts[0] != "docs":
        return "decision.reference must live under docs/"
    if action == "fresh_genesis" and relative != FRESH_GENESIS_RUNBOOK:
        return f"fresh_genesis must reference {FRESH_GENESIS_RUNBOOK}"
    if not (root / relative).is_file():
        return f"decision.reference does not exist: {relative}"
    return None


def check(root: Path) -> list[str]:
    errors = []
    expected_inputs = collect_inputs(root)
    document = load_json(root, DECLARATION)

    if document.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    actual_inputs = document.get("inputs")
    if actual_inputs != expected_inputs:
        if isinstance(actual_inputs, dict):
            changed = sorted(
                key
                for key in set(expected_inputs) | set(actual_inputs)
                if actual_inputs.get(key) != expected_inputs.get(key)
            )
        else:
            changed = ["invalid inputs shape"]
        detail = f" ({', '.join(changed)})" if changed else ""
        errors.append(f"recorded validity inputs are stale{detail}")
    expected_fingerprint = fingerprint(expected_inputs)
    if document.get("fingerprint") != expected_fingerprint:
        errors.append(
            "fingerprint does not bind the current validity inputs "
            f"(expected {expected_fingerprint})"
        )

    decision = document.get("decision")
    if not isinstance(decision, dict):
        errors.append("decision must be an object")
        return errors
    action = decision.get("action")
    if action not in {"fresh_genesis", "migration"}:
        errors.append("decision.action must be fresh_genesis or migration")
    reason = decision.get("reason")
    if not isinstance(reason, str) or len(reason.strip()) < 12:
        errors.append("decision.reason must explain this boundary")
    if action in {"fresh_genesis", "migration"}:
        reference_error = validate_reference(root, action, decision.get("reference"))
        if reference_error:
            errors.append(reference_error)
    return errors


def write(root: Path, action: str, reason: str, reference: str | None) -> None:
    if len(reason.strip()) < 12:
        raise BoundaryError("--reason must explain this boundary (at least 12 chars)")
    if action == "fresh_genesis":
        reference = reference or str(FRESH_GENESIS_RUNBOOK)
    elif not reference:
        raise BoundaryError("--reference is required for a migration")
    reference_error = validate_reference(root, action, reference)
    if reference_error:
        raise BoundaryError(reference_error)

    inputs = collect_inputs(root)
    document = {
        "schema_version": 1,
        "fingerprint": fingerprint(inputs),
        "inputs": inputs,
        "decision": {
            "action": action,
            "reason": reason.strip(),
            "reference": reference,
        },
    }
    path = root / DECLARATION
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {DECLARATION} ({document['fingerprint']}, {action})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=DEFAULT_ROOT, help=argparse.SUPPRESS
    )
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--action", choices=("fresh_genesis", "migration"))
    parser.add_argument("--reason")
    parser.add_argument("--reference")
    args = parser.parse_args()

    try:
        if args.write:
            if not args.action or not args.reason:
                parser.error("--write requires --action and --reason")
            write(args.root.resolve(), args.action, args.reason, args.reference)
            return 0
        errors = check(args.root.resolve())
    except BoundaryError as error:
        errors = [str(error)]

    if errors:
        print("Validity deployment-boundary check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "Refresh validity artifacts first, then explicitly declare the "
            "boundary with `scripts/check-validity-boundary.py --write ...`.",
            file=sys.stderr,
        )
        return 1
    document = load_json(args.root.resolve(), DECLARATION)
    print(
        "validity deployment boundary declared: "
        f"{document['decision']['action']} ({document['fingerprint']})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
