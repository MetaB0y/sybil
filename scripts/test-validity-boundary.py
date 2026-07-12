#!/usr/bin/env python3
"""Fixture tests for check-validity-boundary.py."""

from __future__ import annotations

import importlib.util
import io
import json
import shutil
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path


SCRIPTS = Path(__file__).resolve().parent
FIXTURE = SCRIPTS / "tests/validity-boundary"
SPEC = importlib.util.spec_from_file_location(
    "check_validity_boundary", SCRIPTS / "check-validity-boundary.py"
)
assert SPEC is not None and SPEC.loader is not None
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


class ValidityBoundaryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name) / "repo"
        shutil.copytree(FIXTURE, self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_fresh_genesis(self) -> None:
        with redirect_stdout(io.StringIO()):
            guard.write(
                self.root,
                "fresh_genesis",
                "Fixture validity inputs require a clean chain boundary.",
                None,
            )

    def test_fresh_genesis_declaration_passes(self) -> None:
        self.write_fresh_genesis()
        self.assertEqual(guard.check(self.root), [])

    def test_golden_schema_drift_requires_new_declaration(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "golden/golden-vectors.json"
        golden = json.loads(path.read_text(encoding="utf-8"))
        golden["schema_version"] += 1
        path.write_text(json.dumps(golden), encoding="utf-8")
        errors = guard.check(self.root)
        self.assertTrue(any("golden_vectors_sha256" in error for error in errors))

    def test_commonware_drift_requires_new_declaration(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "Cargo.lock"
        text = path.read_text(encoding="utf-8").replace("2026.5.0", "2026.6.0")
        path.write_text(text, encoding="utf-8")
        errors = guard.check(self.root)
        self.assertTrue(any("commonware_packages" in error for error in errors))

    def test_guest_drift_requires_new_declaration(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "zk/openvm-guest/guest.commitment.lock.json"
        lock = json.loads(path.read_text(encoding="utf-8"))
        lock["source_sha256"] = "changed-source"
        path.write_text(json.dumps(lock), encoding="utf-8")
        errors = guard.check(self.root)
        self.assertTrue(any("guests" in error for error in errors))

    def test_desired_pin_drift_requires_new_declaration(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "deploy/validity-pins.json"
        pins = json.loads(path.read_text(encoding="utf-8"))
        pins["desired"]["state_transition"]["app_exe_commit"] = "new-main-exe"
        path.write_text(json.dumps(pins), encoding="utf-8")
        errors = guard.check(self.root)
        self.assertTrue(any("desired_validity_pins" in error for error in errors))

    def test_deployment_evidence_does_not_move_validity_boundary(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "deploy/validity-pins.json"
        pins = json.loads(path.read_text(encoding="utf-8"))
        pins["status"] = "deployed"
        pins["deployed"] = {"verified_at": "fixture timestamp"}
        path.write_text(json.dumps(pins), encoding="utf-8")
        self.assertEqual(guard.check(self.root), [])

    def test_invalid_decision_is_rejected(self) -> None:
        self.write_fresh_genesis()
        path = self.root / "deploy/validity-boundary.json"
        declaration = json.loads(path.read_text(encoding="utf-8"))
        declaration["decision"]["action"] = "normal_deploy"
        path.write_text(json.dumps(declaration), encoding="utf-8")
        errors = guard.check(self.root)
        self.assertIn(
            "decision.action must be fresh_genesis or migration", errors
        )

    def test_migration_requires_an_existing_plan(self) -> None:
        with self.assertRaisesRegex(guard.BoundaryError, "does not exist"):
            guard.write(
                self.root,
                "migration",
                "Fixture state migrates at an explicit version boundary.",
                "docs/runbooks/missing-migration.md",
            )
        with redirect_stdout(io.StringIO()):
            guard.write(
                self.root,
                "migration",
                "Fixture state migrates at an explicit version boundary.",
                "docs/runbooks/fixture-migration.md",
            )
        self.assertEqual(guard.check(self.root), [])


if __name__ == "__main__":
    unittest.main()
