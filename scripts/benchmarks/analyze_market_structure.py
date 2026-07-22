#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "blake3==1.0.8",
#   "numpy==2.5.1",
# ]
# ///
"""Validate and summarize paired market-structure experiment artifacts."""

from __future__ import annotations

import argparse
import csv
import gzip
import json
import os
import shutil
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator

import blake3
import numpy as np


SCRIPT_SCHEMA_VERSION = 1
TARGET_ENGINE = "sybil-fba"
ANALYSIS_AXES = (
    "batch_interval_ms",
    "quote_half_spread_nanos",
    "maker_reaction_ms",
    "taker_reaction_ms",
    "informed_trader_count",
    "jump_nanos",
    "market_count",
    "budget_fraction_ppm",
    "flow_concentration",
    "shocked_market_count",
    "taker_edge_nanos",
    "mm_budget_fraction_ppm",
)
PROHIBITED_IDENTITY_KEYS = {
    "proxyWallet",
    "name",
    "pseudonym",
    "bio",
    "profileImage",
    "profileImageOptimized",
}


@dataclass(frozen=True)
class MetricSpec:
    direction: str
    threshold_kind: str | None = None


METRICS: dict[str, MetricSpec] = {
    "maker_markout_pnl_nanos": MetricSpec("higher"),
    "maker_pnl_per_filled_share_nanos": MetricSpec("higher", "money"),
    "maker_stale_quote_loss_nanos": MetricSpec("lower"),
    "maker_filled_quantity_units": MetricSpec("higher"),
    "natural_trader_surplus_nanos": MetricSpec("higher"),
    "informed_trader_surplus_nanos": MetricSpec("higher"),
    "submitted_trader_quantity_units": MetricSpec("descriptive"),
    "filled_trader_quantity_units": MetricSpec("higher"),
    "fill_rate_ppm": MetricSpec("higher", "fill_or_coverage"),
    "execution_delay_ms": MetricSpec("lower", "delay"),
    "post_window_price_error_nanos": MetricSpec("lower", "money"),
    "displayed_quote_market_coverage_ppm": MetricSpec(
        "higher", "fill_or_coverage"
    ),
    "single_market_executable_coverage_ppm": MetricSpec(
        "higher", "fill_or_coverage"
    ),
    "simultaneous_worst_case_coverage_ppm": MetricSpec(
        "higher", "fill_or_coverage"
    ),
    "filled_market_coverage_ppm": MetricSpec("higher", "fill_or_coverage"),
    "capital_reserved_nanos": MetricSpec("descriptive"),
    "capital_consumed_nanos": MetricSpec("descriptive"),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate paired FBA/CLOB rows and build the evidence tables"
    )
    parser.add_argument("--protocol", required=True, type=Path)
    parser.add_argument("--runs", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--historical", type=Path)
    parser.add_argument("--historical-manifest", type=Path)
    return parser.parse_args()


def file_blake3(path: Path) -> str:
    digest = blake3.blake3()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def jsonl_rows(path: Path) -> Iterator[dict[str, Any]]:
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8") as source:
        for line_number, line in enumerate(source, start=1):
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(
                    f"{path}:{line_number}: invalid JSON: {error}"
                ) from error
            if not isinstance(row, dict):
                raise ValueError(f"{path}:{line_number}: row is not an object")
            yield row


def expected_engines(row: dict[str, Any]) -> set[str]:
    if row["suite"] == "single-market-microstructure":
        expected = {"clob-firm-reserve", TARGET_ENGINE}
        if row["regime"] == "jump":
            expected.add("fba-cancellable-sensitivity")
        return expected
    if row["suite"] == "shared-budget-portfolio":
        return {"clob-firm-reserve", "clob-shared-risk", TARGET_ENGINE}
    if row["suite"] == "deferred-bundle-lifecycle":
        return {TARGET_ENGINE, "fba-atomic-cancel", "fba-atomic-replace"}
    raise ValueError(f"unknown suite {row['suite']}")


def validate_runs(
    rows: list[dict[str, Any]], protocol: dict[str, Any], protocol_hash: str
) -> dict[str, Any]:
    if not rows:
        raise ValueError("run artifact is empty")
    keys = [(row["suite"], row["case_id"], row["seed"], row["engine"]) for row in rows]
    duplicates = [key for key, count in Counter(keys).items() if count != 1]
    if duplicates:
        raise ValueError(f"duplicate engine rows: {duplicates[:3]}")
    for row in rows:
        if row.get("record_schema_version") != 1:
            raise ValueError("unsupported run record schema")
        if row.get("protocol_id") != protocol["protocol_id"]:
            raise ValueError("run protocol ID does not match analysis protocol")
        if row.get("protocol_blake3") != protocol_hash:
            raise ValueError("run protocol hash does not match analysis protocol")
        metric_keys = set(row.get("metrics", {}))
        if metric_keys != set(METRICS):
            missing = sorted(set(METRICS) - metric_keys)
            extra = sorted(metric_keys - set(METRICS))
            raise ValueError(
                f"run metric schema mismatch; missing={missing}, extra={extra}"
            )

    grouped: dict[tuple[str, str, int], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[(row["suite"], row["case_id"], row["seed"])].append(row)
    seed_sets: dict[tuple[str, str], set[int]] = defaultdict(set)
    for key, group in grouped.items():
        engines = {row["engine"] for row in group}
        expected = expected_engines(group[0])
        if engines != expected:
            raise ValueError(
                f"incomplete engine group {key}: expected {sorted(expected)}, "
                f"got {sorted(engines)}"
            )
        tape_hashes = {row["tape_blake3"] for row in group}
        if len(tape_hashes) != 1:
            raise ValueError(f"unpaired tape hashes for {key}")
        seed_sets[(key[0], key[1])].add(key[2])
    distinct_seed_sets = {tuple(sorted(seeds)) for seeds in seed_sets.values()}
    if len(distinct_seed_sets) != 1:
        raise ValueError("case configurations do not share one complete seed set")

    statuses = Counter((row["engine"], row["run_status"]) for row in rows)
    invalid_verifier_rows = sum(
        any(not evidence["verifier_valid"] for evidence in row["solver_evidence"])
        for row in rows
    )
    return {
        "rows": len(rows),
        "paired_episode_groups": len(grouped),
        "case_configurations": len(seed_sets),
        "seeds": list(next(iter(distinct_seed_sets))),
        "run_status_counts": [
            {"engine": engine, "status": status, "count": count}
            for (engine, status), count in sorted(statuses.items())
        ],
        "invalid_verifier_rows": invalid_verifier_rows,
    }


def comparisons_for_row(row: dict[str, Any]) -> list[tuple[str, str]]:
    if row["suite"] == "single-market-microstructure":
        comparisons = [(TARGET_ENGINE, "clob-firm-reserve")]
        if row["regime"] == "jump":
            comparisons.append(("fba-cancellable-sensitivity", TARGET_ENGINE))
        return comparisons
    if row["suite"] == "deferred-bundle-lifecycle":
        return [
            ("fba-atomic-cancel", TARGET_ENGINE),
            ("fba-atomic-replace", TARGET_ENGINE),
            ("fba-atomic-replace", "fba-atomic-cancel"),
        ]
    return [
        (TARGET_ENGINE, "clob-firm-reserve"),
        (TARGET_ENGINE, "clob-shared-risk"),
    ]


def paired_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for row in rows:
        grouped[(row["suite"], row["case_id"], row["seed"])][row["engine"]] = row
    paired = []
    for (suite, case_id, seed), engines in sorted(grouped.items()):
        exemplar = next(iter(engines.values()))
        for target, comparator in comparisons_for_row(exemplar):
            for metric, spec in METRICS.items():
                target_value = engines[target]["metrics"][metric]
                comparator_value = engines[comparator]["metrics"][metric]
                difference = None
                advantage = None
                if target_value is not None and comparator_value is not None:
                    difference = target_value - comparator_value
                    if spec.direction == "higher":
                        advantage = difference
                    elif spec.direction == "lower":
                        advantage = -difference
                paired.append(
                    {
                        "suite": suite,
                        "regime": exemplar["regime"],
                        "case_id": case_id,
                        **{
                            axis: exemplar["parameters"].get(axis)
                            for axis in ANALYSIS_AXES
                        },
                        "seed": seed,
                        "tape_blake3": exemplar["tape_blake3"],
                        "target_engine": target,
                        "comparator_engine": comparator,
                        "metric": metric,
                        "direction": spec.direction,
                        "target_value": target_value,
                        "comparator_value": comparator_value,
                        "difference_target_minus_comparator": difference,
                        "direction_adjusted_target_advantage": advantage,
                    }
                )
    return paired


def bootstrap_mean_interval(
    values: list[int], *, resamples: int, stable_key: str
) -> tuple[float, float]:
    if not values:
        raise ValueError("cannot bootstrap an empty vector")
    if len(values) == 1:
        point = float(values[0])
        return point, point
    seed = int.from_bytes(blake3.blake3(stable_key.encode()).digest(length=8), "big")
    rng = np.random.Generator(np.random.PCG64(seed))
    vector = np.asarray(values, dtype=np.float64)
    indices = rng.integers(0, len(vector), size=(resamples, len(vector)))
    means = vector[indices].mean(axis=1)
    low, high = np.quantile(means, [0.025, 0.975], method="linear")
    return float(low), float(high)


def materiality_threshold(
    spec: MetricSpec, materiality: dict[str, Any]
) -> int | None:
    if spec.threshold_kind == "money":
        return int(materiality["money_effect_nanos_per_share"])
    if spec.threshold_kind == "fill_or_coverage":
        return int(materiality["fill_or_coverage_effect_ppm"])
    if spec.threshold_kind == "delay":
        return int(materiality["delay_effect_ms"])
    return None


def summarize_pairs(
    pairs: list[dict[str, Any]], protocol: dict[str, Any]
) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, ...], list[dict[str, Any]]] = defaultdict(list)
    for row in pairs:
        key = (
            row["suite"],
            row["regime"],
            row["case_id"],
            row["target_engine"],
            row["comparator_engine"],
            row["metric"],
        )
        grouped[key].append(row)
    resamples = int(protocol["uncertainty"]["bootstrap_resamples"])
    output = []
    for key, rows in sorted(grouped.items()):
        values = [
            row["difference_target_minus_comparator"]
            for row in rows
            if row["difference_target_minus_comparator"] is not None
        ]
        summary: dict[str, Any] = {
            "suite": key[0],
            "regime": key[1],
            "case_id": key[2],
            **{axis: rows[0][axis] for axis in ANALYSIS_AXES},
            "target_engine": key[3],
            "comparator_engine": key[4],
            "metric": key[5],
            "direction": METRICS[key[5]].direction,
            "paired_episode_count": len(rows),
            "defined_pair_count": len(values),
            "undefined_pair_count": len(rows) - len(values),
            "bootstrap_resamples": resamples,
            "mean_difference": None,
            "median_difference": None,
            "q25_difference": None,
            "q75_difference": None,
            "mean_difference_ci95_low": None,
            "mean_difference_ci95_high": None,
            "mean_direction_adjusted_advantage": None,
            "advantage_ci95_low": None,
            "advantage_ci95_high": None,
            "materiality_threshold": None,
            "materiality_classification": "undefined",
        }
        if values:
            vector = np.asarray(values, dtype=np.float64)
            stable_key = "|".join((protocol["protocol_id"], *key))
            ci_low, ci_high = bootstrap_mean_interval(
                values, resamples=resamples, stable_key=stable_key
            )
            mean = float(vector.mean())
            q25, median, q75 = np.quantile(
                vector, [0.25, 0.5, 0.75], method="linear"
            )
            direction = METRICS[key[5]].direction
            if direction == "higher":
                advantage = mean
                advantage_low, advantage_high = ci_low, ci_high
            elif direction == "lower":
                advantage = -mean
                advantage_low, advantage_high = -ci_high, -ci_low
            else:
                advantage = None
                advantage_low = advantage_high = None
            threshold = materiality_threshold(
                METRICS[key[5]], protocol["materiality"]
            )
            classification = "no_declared_threshold"
            if threshold is not None and advantage is not None:
                if advantage > threshold and advantage_low > 0:
                    classification = "material_target_advantage"
                elif advantage < -threshold and advantage_high < 0:
                    classification = "material_target_disadvantage"
                else:
                    classification = "not_material_by_preregistered_rule"
            summary.update(
                {
                    "mean_difference": mean,
                    "median_difference": float(median),
                    "q25_difference": float(q25),
                    "q75_difference": float(q75),
                    "mean_difference_ci95_low": ci_low,
                    "mean_difference_ci95_high": ci_high,
                    "mean_direction_adjusted_advantage": advantage,
                    "advantage_ci95_low": advantage_low,
                    "advantage_ci95_high": advantage_high,
                    "materiality_threshold": threshold,
                    "materiality_classification": classification,
                }
            )
        output.append(summary)
    return output


def flatten_engine_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    output = []
    for row in sorted(
        rows,
        key=lambda row: (
            row["suite"],
            row["case_id"],
            row["seed"],
            row["engine"],
        ),
    ):
        flattened = {
            "suite": row["suite"],
            "regime": row["regime"],
            "case_id": row["case_id"],
            **{axis: row["parameters"].get(axis) for axis in ANALYSIS_AXES},
            "seed": row["seed"],
            "tape_blake3": row["tape_blake3"],
            "engine": row["engine"],
            "run_status": row["run_status"],
            "solver_attempt_count": len(row["solver_evidence"]),
            "invalid_verifier_attempt_count": sum(
                not evidence["verifier_valid"] for evidence in row["solver_evidence"]
            ),
        }
        flattened.update(row["metrics"])
        output.append(flattened)
    return output


def recursively_reject_identity_keys(value: Any) -> None:
    if isinstance(value, dict):
        found = PROHIBITED_IDENTITY_KEYS.intersection(value)
        if found:
            raise ValueError(
                f"historical artifact retained identity keys: {sorted(found)}"
            )
        for child in value.values():
            recursively_reject_identity_keys(child)
    elif isinstance(value, list):
        for child in value:
            recursively_reject_identity_keys(child)


def analyze_historical(
    path: Path,
    manifest_path: Path,
    protocol: dict[str, Any],
    protocol_hash: str,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    manifest = json.loads(manifest_path.read_bytes())
    if manifest["artifact"]["blake3"] != file_blake3(path):
        raise ValueError("historical artifact hash does not match its manifest")
    if manifest["protocol"]["blake3"] != protocol_hash:
        raise ValueError("historical artifact was not captured from this protocol")
    rows = list(jsonl_rows(path))
    for row in rows:
        recursively_reject_identity_keys(row)
    markets = [row for row in rows if row.get("record_type") == "market"]
    transactions = [
        row for row in rows if row.get("record_type") == "taker_transaction"
    ]
    if len(markets) != 31 or len({row["condition_id"] for row in markets}) != 31:
        raise ValueError("historical artifact does not contain 31 unique markets")
    if len(transactions) != manifest["completeness"]["taker_transactions"]:
        raise ValueError("historical transaction count does not match manifest")

    expected_known = {
        value.lower()
        for value in protocol["historical_case_study"]["known_transaction_hashes"]
    }
    found_known = {
        row["transaction_hash"] for row in transactions if row["known_case"]
    }
    if found_known != expected_known:
        raise ValueError("historical known-case labels do not match the protocol")

    flattened = []
    for row in transactions:
        reconstruction = row["counterpart_reconstruction"]
        taker = row["taker"]
        flattened.append(
            {
                "market_slug": row["market_slug"],
                "condition_id": row["condition_id"],
                "transaction_hash": row["transaction_hash"],
                "timestamp": row["timestamp"],
                "known_case": row["known_case"],
                "settlement_yes_nanos": row["settlement_yes_nanos"],
                "taker_effective_yes_side": taker["effective_yes_side"],
                "taker_effective_yes_price_nanos": taker[
                    "effective_yes_price_nanos"
                ],
                "taker_quantity_microshares": taker["quantity_microshares"],
                "taker_gross_settlement_markout_nanos": taker[
                    "gross_settlement_markout_nanos"
                ],
                "counterpart_reconstruction_status": reconstruction["status"],
                "counterpart_row_count": reconstruction["row_count"],
                "counterpart_quantity_microshares": reconstruction[
                    "quantity_microshares"
                ],
                "counterpart_quantity_delta_microshares": reconstruction[
                    "quantity_delta_microshares"
                ],
                "counterpart_effective_yes_price_min_nanos": reconstruction[
                    "effective_yes_price_min_nanos"
                ],
                "counterpart_effective_yes_price_max_nanos": reconstruction[
                    "effective_yes_price_max_nanos"
                ],
                "counterpart_effective_yes_weighted_price_nanos": reconstruction[
                    "effective_yes_weighted_price_nanos"
                ],
                "counterpart_gross_settlement_markout_nanos": reconstruction[
                    "gross_settlement_markout_nanos"
                ],
            }
        )
    flattened.sort(key=lambda row: (row["timestamp"], row["transaction_hash"]))
    exact = [
        row
        for row in flattened
        if row["counterpart_reconstruction_status"] == "exact"
    ]
    known_rows = [row for row in flattened if row["known_case"]]
    quantity_ranking = sorted(
        exact, key=lambda row: row["taker_quantity_microshares"], reverse=True
    )
    loss_ranking = sorted(
        exact,
        key=lambda row: -(row["counterpart_gross_settlement_markout_nanos"] or 0),
        reverse=True,
    )
    quantity_rank = {
        row["transaction_hash"]: rank
        for rank, row in enumerate(quantity_ranking, start=1)
    }
    loss_rank = {
        row["transaction_hash"]: rank
        for rank, row in enumerate(loss_ranking, start=1)
    }
    summary = {
        "markets": len(markets),
        "transactions": len(flattened),
        "exact_reconstructions": len(exact),
        "unreconciled_reconstructions": len(flattened) - len(exact),
        "taker_quantity_microshares": sum(
            row["taker_quantity_microshares"] for row in flattened
        ),
        "counterpart_gross_settlement_markout_nanos_exact": sum(
            row["counterpart_gross_settlement_markout_nanos"] for row in exact
        ),
        "exact_counterpart_loss_transaction_count": sum(
            row["counterpart_gross_settlement_markout_nanos"] < 0 for row in exact
        ),
        "known_cases": [
            {
                **row,
                "quantity_rank_among_exact": quantity_rank[row["transaction_hash"]],
                "counterpart_loss_rank_among_exact": loss_rank[
                    row["transaction_hash"]
                ],
            }
            for row in known_rows
        ],
        "interpretation_boundary": {
            "resting_rows_are_classified_as_market_makers": False,
            "historical_fba_counterfactual_identified": False,
            "fees_hedges_rebates_inventory_observed": False,
        },
    }
    return flattened, summary


def csv_bytes(rows: list[dict[str, Any]]) -> bytes:
    if not rows:
        raise ValueError("refusing to write an empty CSV")
    import io

    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=list(rows[0]), lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue().encode()


def write_bytes(path: Path, content: bytes) -> None:
    with path.open("wb") as output:
        output.write(content)
        output.flush()
        os.fsync(output.fileno())


def summary_markdown(
    protocol: dict[str, Any],
    validation: dict[str, Any],
    historical: dict[str, Any] | None,
) -> str:
    evidence_label = (
        "publishable held-out evidence"
        if protocol["status"] == "frozen-held-out-evidence"
        else "development diagnostic; not evidence"
    )
    lines = [
        "# Market-structure analysis artifact",
        "",
        f"Protocol: `{protocol['protocol_id']}` ({evidence_label}).",
        "",
        f"Validated {validation['rows']} engine rows across "
        f"{validation['paired_episode_groups']} paired episode groups and "
        f"{validation['case_configurations']} configurations.",
        "",
        "`paired-summary.csv` is the complete configuration/metric table. Positive "
        "direction-adjusted effects favor the target engine. Undefined conditional "
        "metrics remain undefined; fill-rate metrics retain zero-fill episodes.",
        "",
        "Materiality classifications apply only where the protocol declares a "
        "threshold and never combine maker, trader, coverage, price, or delay metrics.",
    ]
    if historical is not None:
        lines.extend(
            [
                "",
                "## Historical case study",
                "",
                f"The identity-free tape contains {historical['transactions']} taker "
                f"transactions across {historical['markets']} markets; "
                f"{historical['exact_reconstructions']} reconcile exactly and "
                f"{historical['unreconciled_reconstructions']} do not.",
                "",
                "Resting-side rows are anonymous executions, not identified market "
                "makers. Settlement markouts exclude fees, hedges, rebates, and prior "
                "inventory and do not identify an FBA counterfactual.",
            ]
        )
    return "\n".join(lines) + "\n"


def build(args: argparse.Namespace) -> None:
    if args.output_dir.exists():
        raise FileExistsError(f"output directory already exists: {args.output_dir}")
    if (args.historical is None) != (args.historical_manifest is None):
        raise ValueError(
            "--historical and --historical-manifest must be supplied together"
        )
    protocol_bytes = args.protocol.read_bytes()
    protocol = json.loads(protocol_bytes)
    protocol_hash = blake3.blake3(protocol_bytes).hexdigest()
    rows = list(jsonl_rows(args.runs))
    validation = validate_runs(rows, protocol, protocol_hash)
    pairs = paired_rows(rows)
    summaries = summarize_pairs(pairs, protocol)

    historical_rows = None
    historical_summary = None
    if args.historical is not None and args.historical_manifest is not None:
        historical_rows, historical_summary = analyze_historical(
            args.historical,
            args.historical_manifest,
            protocol,
            protocol_hash,
        )

    temporary = args.output_dir.with_name(f".{args.output_dir.name}.tmp-{os.getpid()}")
    temporary.mkdir(parents=True)
    try:
        write_bytes(
            temporary / "engine-metrics.csv",
            csv_bytes(flatten_engine_rows(rows)),
        )
        write_bytes(temporary / "paired-differences.csv", csv_bytes(pairs))
        write_bytes(temporary / "paired-summary.csv", csv_bytes(summaries))
        if historical_rows is not None and historical_summary is not None:
            write_bytes(
                temporary / "historical-transactions.csv", csv_bytes(historical_rows)
            )
            write_bytes(
                temporary / "historical-summary.json",
                (
                    json.dumps(historical_summary, indent=2, sort_keys=True) + "\n"
                ).encode(),
            )
        analysis_manifest = {
            "schema_version": SCRIPT_SCHEMA_VERSION,
            "protocol_id": protocol["protocol_id"],
            "protocol_status": protocol["status"],
            "protocol_blake3": protocol_hash,
            "runs": {"path": str(args.runs), "blake3": file_blake3(args.runs)},
            "historical": (
                None
                if args.historical is None
                else {
                    "path": str(args.historical),
                    "blake3": file_blake3(args.historical),
                    "manifest_path": str(args.historical_manifest),
                    "manifest_blake3": file_blake3(args.historical_manifest),
                }
            ),
            "validation": validation,
            "paired_difference_rows": len(pairs),
            "paired_summary_rows": len(summaries),
            "bootstrap": {
                "resamples": protocol["uncertainty"]["bootstrap_resamples"],
                "cluster": "independent paired episode seed within configuration",
                "rng": (
                    "numpy PCG64 seeded by "
                    "BLAKE3(protocol, comparison, case, metric)"
                ),
                "interval": (
                    "2.5th and 97.5th linear-interpolated percentiles "
                    "of paired means"
                ),
            },
        }
        write_bytes(
            temporary / "analysis-manifest.json",
            (json.dumps(analysis_manifest, indent=2, sort_keys=True) + "\n").encode(),
        )
        write_bytes(
            temporary / "README.md",
            summary_markdown(protocol, validation, historical_summary).encode(),
        )
        temporary.replace(args.output_dir)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def main() -> None:
    build(parse_args())


if __name__ == "__main__":
    main()
