#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("store-manifest.py")
SPEC = importlib.util.spec_from_file_location("store_manifest", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
store_manifest = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(store_manifest)


class StoreManifestTests(unittest.TestCase):
    def account(self) -> dict[str, object]:
        return {"account_id": 42, "balance_nanos": 7}

    def v2_manifest(self) -> dict[str, object]:
        return {
            "schema": store_manifest.SCHEMA_V2,
            "expected": {
                "height": 9,
                "committed_state_root": "a" * 64,
                "replayed_state_root": "b" * 64,
                "account_id": 42,
                "account": self.account(),
            },
        }

    def run_compare(
        self,
        manifest: dict[str, object],
        latest_root: str,
        replayed_root: str,
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            values = {
                "manifest": manifest,
                "latest": {"height": 9, "state_root": latest_root},
                "state-root": {"state_root": replayed_root},
                "account": self.account(),
            }
            paths = {}
            for name, value in values.items():
                path = root / f"{name}.json"
                path.write_text(json.dumps(value), encoding="utf-8")
                paths[name] = path
            return subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "compare",
                    "--manifest",
                    str(paths["manifest"]),
                    "--latest",
                    str(paths["latest"]),
                    "--state-root",
                    str(paths["state-root"]),
                    "--account",
                    str(paths["account"]),
                ],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_v2_accepts_distinct_committed_and_replayed_roots(self) -> None:
        result = self.run_compare(self.v2_manifest(), "a" * 64, "b" * 64)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("committed_state_root=" + "a" * 64, result.stdout)
        self.assertIn("replayed_state_root=" + "b" * 64, result.stdout)

    def test_v2_fails_closed_when_either_root_mismatches(self) -> None:
        for latest_root, replayed_root, message in (
            ("c" * 64, "b" * 64, "committed state_root mismatch"),
            ("a" * 64, "c" * 64, "replayed state_root mismatch"),
        ):
            with self.subTest(message=message):
                result = self.run_compare(
                    self.v2_manifest(), latest_root, replayed_root
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)

    def test_v1_preserves_single_root_contract(self) -> None:
        manifest = {
            "schema": store_manifest.SCHEMA_V1,
            "expected": {
                "height": 9,
                "state_root": "a" * 64,
                "account_id": 42,
                "account": self.account(),
            },
        }
        self.assertEqual(
            self.run_compare(manifest, "a" * 64, "a" * 64).returncode, 0
        )
        result = self.run_compare(manifest, "a" * 64, "b" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("replayed state_root mismatch", result.stderr)

    def test_rejects_unknown_schema_and_malformed_roots(self) -> None:
        manifest = self.v2_manifest()
        manifest["schema"] = "sybil.store-backup.v3"
        with self.assertRaises(store_manifest.ManifestError):
            store_manifest.validate_manifest(manifest)

        manifest = self.v2_manifest()
        manifest["expected"]["replayed_state_root"] = (  # type: ignore[index]
            "not-a-root"
        )
        with self.assertRaises(store_manifest.ManifestError):
            store_manifest.validate_manifest(manifest)


if __name__ == "__main__":
    unittest.main()
