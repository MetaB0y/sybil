#!/usr/bin/env python3
"""Validate the bounded Schemathesis operation allowlist."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ALLOWLIST = ROOT / "scripts" / "api-contract-allowlist.json"
HTTP_METHODS = {"get", "put", "post", "delete", "options", "head", "patch", "trace"}
PHASES = {"positive", "negative"}
ENTRY_FIELDS = {"operation_id", "phases", "reason", "owner", "expires_on"}


def load_json(source: str) -> Any:
    if source.startswith(("http://", "https://")):
        with urllib.request.urlopen(source, timeout=10) as response:  # noqa: S310
            return json.load(response)
    return json.loads(Path(source).read_text())


def operation_ids(openapi: Any) -> set[str]:
    found: set[str] = set()
    for path_item in openapi.get("paths", {}).values():
        for method, operation in path_item.items():
            if method in HTTP_METHODS and isinstance(operation, dict):
                operation_id = operation.get("operationId")
                if isinstance(operation_id, str) and operation_id:
                    found.add(operation_id)
    return found


def validate(document: Any, known_operations: set[str]) -> list[dict[str, Any]]:
    failures: list[str] = []
    if not isinstance(document, dict) or set(document) != {"schema_version", "entries"}:
        failures.append("top level must contain only schema_version and entries")
    if document.get("schema_version") != 1:
        failures.append("schema_version must be 1")
    entries = document.get("entries")
    if not isinstance(entries, list):
        failures.append("entries must be an array")
        entries = []

    today = dt.date.today()
    seen: set[str] = set()
    for index, entry in enumerate(entries):
        prefix = f"entries[{index}]"
        if not isinstance(entry, dict):
            failures.append(f"{prefix} must be an object")
            continue
        if set(entry) != ENTRY_FIELDS:
            failures.append(f"{prefix} must contain exactly {sorted(ENTRY_FIELDS)}")
        operation_id = entry.get("operation_id")
        if not isinstance(operation_id, str) or not operation_id:
            failures.append(f"{prefix}.operation_id must be a non-empty string")
        elif operation_id in seen:
            failures.append(f"{prefix}.operation_id duplicates {operation_id!r}")
        else:
            seen.add(operation_id)
            if operation_id not in known_operations:
                failures.append(f"{prefix}.operation_id {operation_id!r} is absent from OpenAPI")
        phases = entry.get("phases")
        if (
            not isinstance(phases, list)
            or not phases
            or len(phases) != len(set(phases))
            or not set(phases).issubset(PHASES)
        ):
            failures.append(f"{prefix}.phases must be unique values from {sorted(PHASES)}")
        reason = entry.get("reason")
        if not isinstance(reason, str) or len(reason.strip()) < 40:
            failures.append(f"{prefix}.reason must be a durable explanation of at least 40 characters")
        owner = entry.get("owner")
        if not isinstance(owner, str) or not owner.strip():
            failures.append(f"{prefix}.owner must be a non-empty string")
        try:
            expires_on = dt.date.fromisoformat(entry.get("expires_on", ""))
            if expires_on < today:
                failures.append(f"{prefix}.expires_on expired on {expires_on.isoformat()}")
        except (TypeError, ValueError):
            failures.append(f"{prefix}.expires_on must be an ISO date")

    if failures:
        print("API contract allowlist validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)
    return entries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--openapi", required=True, help="OpenAPI URL or JSON file")
    parser.add_argument("--allowlist", default=str(DEFAULT_ALLOWLIST))
    parser.add_argument("--phase", choices=sorted(PHASES))
    args = parser.parse_args()

    entries = validate(load_json(args.allowlist), operation_ids(load_json(args.openapi)))
    if args.phase:
        for entry in entries:
            if args.phase in entry["phases"]:
                print(entry["operation_id"])
    else:
        print(f"API contract allowlist: {len(entries)} current entries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
