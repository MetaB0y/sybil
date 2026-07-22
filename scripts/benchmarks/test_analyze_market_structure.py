"""Offline unit tests for paired market-structure analysis."""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path

import blake3


SCRIPT = Path(__file__).with_name("analyze_market_structure.py")
SPEC = importlib.util.spec_from_file_location("analyze_market_structure", SCRIPT)
assert SPEC and SPEC.loader
analysis = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = analysis
SPEC.loader.exec_module(analysis)


def protocol() -> dict:
    return {
        "protocol_id": "test-protocol",
        "status": "development-only-not-evidence",
        "uncertainty": {"bootstrap_resamples": 100},
        "materiality": {
            "money_effect_nanos_per_share": 10_000_000,
            "fill_or_coverage_effect_ppm": 50_000,
            "delay_effect_ms": 250,
        },
    }


def metric_values(fill_rate: int = 0) -> dict:
    values = {metric: 0 for metric in analysis.METRICS}
    values["fill_rate_ppm"] = fill_rate
    return values


def run_row(engine: str, protocol_hash: str, *, seed: int = 7, fill_rate: int = 0):
    return {
        "record_schema_version": 1,
        "protocol_id": "test-protocol",
        "protocol_blake3": protocol_hash,
        "suite": "single-market-microstructure",
        "case_id": "quiet-case",
        "seed": seed,
        "tape_blake3": f"tape-{seed}",
        "engine": engine,
        "regime": "quiet",
        "parameters": {
            "batch_interval_ms": 500,
            "quote_half_spread_nanos": 10_000_000,
        },
        "run_status": "completed",
        "solver_evidence": [],
        "metrics": metric_values(fill_rate),
    }


class AnalysisTests(unittest.TestCase):
    def setUp(self):
        self.protocol = protocol()
        encoded = json.dumps(self.protocol).encode()
        self.protocol_hash = blake3.blake3(encoded).hexdigest()

    def test_validation_rejects_incomplete_engine_group(self):
        rows = [run_row("sybil-fba", self.protocol_hash)]
        with self.assertRaisesRegex(ValueError, "incomplete engine group"):
            analysis.validate_runs(rows, self.protocol, self.protocol_hash)

    def test_materiality_uses_paired_direction_adjusted_effect(self):
        rows = []
        for seed in range(4):
            rows.extend(
                [
                    run_row("clob-firm-reserve", self.protocol_hash, seed=seed),
                    run_row(
                        "sybil-fba",
                        self.protocol_hash,
                        seed=seed,
                        fill_rate=100_000,
                    ),
                ]
            )
        analysis.validate_runs(rows, self.protocol, self.protocol_hash)
        summaries = analysis.summarize_pairs(
            analysis.paired_rows(rows), self.protocol
        )
        fill_summary = next(
            row for row in summaries if row["metric"] == "fill_rate_ppm"
        )
        self.assertEqual(fill_summary["mean_difference"], 100_000)
        self.assertEqual(
            fill_summary["materiality_classification"],
            "material_target_advantage",
        )

    def test_bootstrap_is_stable_for_same_key(self):
        first = analysis.bootstrap_mean_interval(
            [1, 2, 3, 4], resamples=100, stable_key="stable"
        )
        second = analysis.bootstrap_mean_interval(
            [1, 2, 3, 4], resamples=100, stable_key="stable"
        )
        self.assertEqual(first, second)

    def test_historical_identity_keys_fail_closed(self):
        with self.assertRaisesRegex(ValueError, "identity keys"):
            analysis.recursively_reject_identity_keys(
                {"counterpart": {"proxyWallet": "0xsecret"}}
            )


if __name__ == "__main__":
    unittest.main()
