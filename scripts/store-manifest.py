#!/usr/bin/env python3
"""Build and verify crash-consistent Sybil store backup manifests."""

from __future__ import annotations

import argparse
import json
import socket
import sys
from pathlib import Path
from typing import Any


SCHEMA_V1 = "sybil.store-backup.v1"
SCHEMA_V2 = "sybil.store-backup.v2"


class ManifestError(ValueError):
    pass


def load_json(path: str | Path) -> Any:
    with Path(path).open(encoding="utf-8") as handle:
        return json.load(handle)


def require_root(value: Any, field: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdefABCDEF" for character in value)
    ):
        raise ManifestError(f"{field} must be a 64-character hexadecimal string")
    return value


def expected_roots(manifest: dict[str, Any]) -> tuple[str, str]:
    expected = manifest.get("expected")
    if not isinstance(expected, dict):
        raise ManifestError("expected must be an object")

    schema = manifest.get("schema")
    if schema == SCHEMA_V1:
        root = require_root(expected.get("state_root"), "expected.state_root")
        return root, root
    if schema == SCHEMA_V2:
        return (
            require_root(
                expected.get("committed_state_root"),
                "expected.committed_state_root",
            ),
            require_root(
                expected.get("replayed_state_root"),
                "expected.replayed_state_root",
            ),
        )
    raise ManifestError(f"unsupported backup manifest schema: {schema!r}")


def validate_manifest(manifest: Any) -> dict[str, Any]:
    if not isinstance(manifest, dict):
        raise ManifestError("manifest must be an object")
    expected_roots(manifest)

    expected = manifest["expected"]
    height = expected.get("height")
    account_id = expected.get("account_id")
    account = expected.get("account")
    if not isinstance(height, int) or isinstance(height, bool) or height < 0:
        raise ManifestError("expected.height must be a non-negative integer")
    if (
        not isinstance(account_id, int)
        or isinstance(account_id, bool)
        or account_id < 0
    ):
        raise ManifestError("expected.account_id must be a non-negative integer")
    if not isinstance(account, dict) or account.get("account_id") != account_id:
        raise ManifestError("expected.account must match expected.account_id")
    return manifest


def build_manifest(args: argparse.Namespace) -> None:
    latest = load_json(args.latest)
    state_root = load_json(args.state_root)
    account = load_json(args.account)
    if not isinstance(latest, dict) or not isinstance(state_root, dict):
        raise ManifestError("inspector endpoints must return JSON objects")

    height = latest.get("height")
    committed_root = require_root(latest.get("state_root"), "latest.state_root")
    replayed_root = require_root(state_root.get("state_root"), "state-root.state_root")
    if not isinstance(height, int) or isinstance(height, bool) or height < 0:
        raise ManifestError("latest.height must be a non-negative integer")
    if not isinstance(account, dict) or not isinstance(account.get("account_id"), int):
        raise ManifestError("inspector returned an invalid account sample")

    manifest = {
        "schema": SCHEMA_V2,
        "created_utc": args.stamp,
        "host": socket.gethostname(),
        "source": {
            "target": args.target,
            "compose_project": args.project or None,
            "container": args.container,
            "image": args.image,
            "data_dir": args.data_dir,
        },
        "consistency": {
            "mechanism": "docker-pause-whole-container",
            "scope": "complete-sybil-data-dir",
        },
        "expected": {
            "height": height,
            "committed_state_root": committed_root,
            "replayed_state_root": replayed_root,
            "account_id": account["account_id"],
            "account": account,
        },
    }
    validate_manifest(manifest)
    with Path(args.output).open("w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
        handle.write("\n")


def compare_restored(args: argparse.Namespace) -> None:
    manifest = validate_manifest(load_json(args.manifest))
    latest = load_json(args.latest)
    state_root = load_json(args.state_root)
    account = load_json(args.account)
    expected = manifest["expected"]
    committed_root, replayed_root = expected_roots(manifest)

    failures = []
    if not isinstance(latest, dict) or latest.get("height") != expected["height"]:
        actual_height = latest.get("height") if isinstance(latest, dict) else None
        failures.append(f"height expected {expected['height']}, got {actual_height}")
    if not isinstance(latest, dict) or latest.get("state_root") != committed_root:
        failures.append("latest block committed state_root mismatch")
    if (
        not isinstance(state_root, dict)
        or state_root.get("state_root") != replayed_root
    ):
        failures.append("/v1/state-root replayed state_root mismatch")
    if account != expected["account"]:
        failures.append(f"account {expected['account_id']} state mismatch")
    if failures:
        raise ManifestError("; ".join(failures))

    print(
        f"OK: restored height={expected['height']} "
        f"committed_state_root={committed_root} "
        f"replayed_state_root={replayed_root} account={expected['account_id']}"
    )


def command_validate(args: argparse.Namespace) -> None:
    validate_manifest(load_json(args.manifest))


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    subparsers = result.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build")
    build.add_argument("--latest", required=True)
    build.add_argument("--state-root", required=True)
    build.add_argument("--account", required=True)
    build.add_argument("--output", required=True)
    build.add_argument("--stamp", required=True)
    build.add_argument("--target", required=True)
    build.add_argument("--project", default="")
    build.add_argument("--container", required=True)
    build.add_argument("--image", required=True)
    build.add_argument("--data-dir", required=True)
    build.set_defaults(func=build_manifest)

    validate = subparsers.add_parser("validate")
    validate.add_argument("manifest")
    validate.set_defaults(func=command_validate)

    compare = subparsers.add_parser("compare")
    compare.add_argument("--manifest", required=True)
    compare.add_argument("--latest", required=True)
    compare.add_argument("--state-root", required=True)
    compare.add_argument("--account", required=True)
    compare.set_defaults(func=compare_restored)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        args.func(args)
    except (ManifestError, json.JSONDecodeError, OSError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
