#!/usr/bin/env python3
"""Validate and summarize the preregistered solver experiment output.

Only Python's standard library is used. The script refuses incomplete or
duplicate full-protocol data by default, retains every failure in denominators,
and writes tables plus deterministic SVG figures.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import html
import json
import math
import random
import statistics
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

BOOTSTRAP_SEED = 20260713
BOOTSTRAP_RESAMPLES = 10_000

COLORS = {
    "lp": "#111827",
    "iter-lp": "#d97706",
    "eg": "#7c3aed",
    "conic-quasi": "#1d4ed8",
    "conic-fisher": "#0891b2",
    "decomposed-lp": "#be123c",
    "decomposed-quasi": "#059669",
    "milp-exact": "#4b5563",
}

SHORT_LABELS = {
    "lp": "LP",
    "iter-lp": "IterLP",
    "eg": "EG-FW",
    "conic-quasi": "Quasi",
    "conic-fisher": "Fisher",
    "decomposed-lp": "D-LP",
    "decomposed-quasi": "D-Quasi",
    "milp-exact": "MILP",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_dir", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--allow-incomplete", action="store_true")
    return parser.parse_args()


def quantile(values: Iterable[float], probability: float) -> float | None:
    ordered = sorted(values)
    if not ordered:
        return None
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * probability
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def median(values: Iterable[float]) -> float | None:
    materialized = list(values)
    return statistics.median(materialized) if materialized else None


def mean(values: Iterable[float]) -> float | None:
    materialized = list(values)
    return statistics.fmean(materialized) if materialized else None


def bootstrap_mean_interval(values: list[float], key: str) -> tuple[float, float] | None:
    if not values:
        return None
    if len(values) == 1:
        return values[0], values[0]
    digest = hashlib.blake2b(key.encode(), digest_size=8).digest()
    seed = BOOTSTRAP_SEED ^ int.from_bytes(digest, "little")
    rng = random.Random(seed)
    size = len(values)
    samples = [
        statistics.fmean(values[rng.randrange(size)] for _ in range(size))
        for _ in range(BOOTSTRAP_RESAMPLES)
    ]
    return quantile(samples, 0.025), quantile(samples, 0.975)  # type: ignore[return-value]


def expected_keys(protocol: dict[str, Any]) -> set[tuple[str, int, float, str]]:
    keys = set()
    for experiment in protocol["experiments"]:
        for seed in range(
            experiment["seed_start"], experiment["seed_start"] + experiment["seed_count"]
        ):
            for budget_scale in experiment["budget_scales"]:
                for solver in experiment["solvers"]:
                    keys.add((experiment["id"], seed, float(budget_scale), solver))
    return keys


def load_and_validate(
    input_dir: Path, allow_incomplete: bool
) -> tuple[dict[str, Any], dict[str, Any], list[dict[str, Any]], dict[str, Any]]:
    protocol = json.loads((input_dir / "protocol.json").read_text())
    metadata = json.loads((input_dir / "metadata.json").read_text())
    records = [json.loads(line) for line in (input_dir / "results.jsonl").read_text().splitlines()]

    actual_keys = [
        (row["experiment_id"], row["seed"], float(row["budget_scale"]), row["solver_id"])
        for row in records
    ]
    duplicates = [key for key, count in Counter(actual_keys).items() if count > 1]
    if duplicates:
        raise ValueError(f"duplicate run keys: {duplicates[:5]}")

    expected = expected_keys(protocol)
    actual = set(actual_keys)
    missing = sorted(expected - actual)
    unexpected = sorted(actual - expected)
    if unexpected:
        raise ValueError(f"unexpected run keys: {unexpected[:5]}")
    if not allow_incomplete and missing:
        raise ValueError(f"missing {len(missing)} declared runs; first: {missing[:5]}")
    if not allow_incomplete and not metadata.get("protocol_complete"):
        raise ValueError("metadata does not mark the protocol complete")

    fingerprints: dict[tuple[str, int, float], set[str]] = defaultdict(set)
    for row in records:
        fingerprints[(row["experiment_id"], row["seed"], float(row["budget_scale"]))].add(
            row["scenario_fingerprint_blake3"]
        )
    mismatched = [key for key, values in fingerprints.items() if len(values) != 1]
    if mismatched:
        raise ValueError(f"solvers did not share identical problems: {mismatched[:5]}")

    integrity = {
        "declared_records": len(expected),
        "observed_records": len(records),
        "missing_records": len(missing),
        "unexpected_records": len(unexpected),
        "duplicate_records": len(duplicates),
        "scenario_groups": len(fingerprints),
        "scenario_fingerprint_mismatches": len(mismatched),
        "complete": not missing and not unexpected and not duplicates and not mismatched,
    }
    return protocol, metadata, records, integrity


def aggregate(records: list[dict[str, Any]], dimensions: tuple[str, ...]) -> list[dict[str, Any]]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = defaultdict(list)
    for row in records:
        grouped[tuple(row[field] for field in dimensions)].append(row)

    result = []
    for key, rows in sorted(grouped.items()):
        successful = [row for row in rows if row["benchmark_success"]]
        gaps = [
            row["comparisons"]["lp_welfare_gap_bps"] / 100.0
            for row in successful
            if row["comparisons"]["lp_welfare_gap_bps"] is not None
        ]
        runtimes = [row["wall_time_seconds"] for row in successful]
        allocation = [
            row["comparisons"]["lp_allocation_l1_ratio"]
            for row in successful
            if row["comparisons"]["lp_allocation_l1_ratio"] is not None
        ]
        convergence = Counter(row["termination"] for row in rows)
        statuses = Counter(row["run_status"] for row in rows)
        item = {field: value for field, value in zip(dimensions, key)}
        item.update(
            {
                "declared": len(rows),
                "successful": len(successful),
                "failed": len(rows) - len(successful),
                "verifier_invalid": sum(not row["verifier_valid"] for row in rows),
                "status_counts": dict(sorted(statuses.items())),
                "termination_counts": dict(sorted(convergence.items())),
                "runtime_median_seconds": median(runtimes),
                "runtime_p25_seconds": quantile(runtimes, 0.25),
                "runtime_p75_seconds": quantile(runtimes, 0.75),
                "lp_gap_mean_percent": mean(gaps),
                "lp_gap_median_percent": median(gaps),
                "lp_gap_p25_percent": quantile(gaps, 0.25),
                "lp_gap_p75_percent": quantile(gaps, 0.75),
                "lp_gap_min_percent": min(gaps) if gaps else None,
                "lp_gap_max_percent": max(gaps) if gaps else None,
                "allocation_l1_median": median(allocation),
            }
        )
        interval = bootstrap_mean_interval(gaps, json.dumps(key))
        item["lp_gap_mean_ci95_percent"] = list(interval) if interval else None
        result.append(item)
    return result


def make_summary(
    protocol: dict[str, Any], metadata: dict[str, Any], records: list[dict[str, Any]], integrity: dict[str, Any]
) -> dict[str, Any]:
    quality = [row for row in records if row["suite"] == "quality"]
    scaling = [row for row in records if row["suite"] == "scaling"]
    budget = [row for row in records if row["suite"] == "budget"]
    decomposition = [row for row in records if row["suite"] == "decomposition"]
    reference = [row for row in records if row["suite"] == "reference"]
    return {
        "schema_version": 1,
        "protocol_id": protocol["protocol_id"],
        "source_revision": metadata["source_revision"],
        "integrity": integrity,
        "overall": aggregate(records, ("solver_id",)),
        "quality": aggregate(quality, ("profile", "solver_id")),
        "scaling": aggregate(scaling, ("scale", "solver_id")),
        "budget": aggregate(budget, ("budget_scale", "solver_id")),
        "decomposition": aggregate(decomposition, ("profile", "solver_id")),
        "reference": aggregate(reference, ("solver_id",)),
    }


def format_number(value: float | int | None, digits: int = 3) -> str:
    if value is None:
        return "—"
    if isinstance(value, int):
        return str(value)
    return f"{value:.{digits}f}"


def markdown_table(headers: list[str], rows: list[list[str]]) -> str:
    lines = ["| " + " | ".join(headers) + " |", "|" + "|".join("---" for _ in headers) + "|"]
    lines.extend("| " + " | ".join(row) + " |" for row in rows)
    return "\n".join(lines)


def write_markdown(summary: dict[str, Any], output: Path) -> None:
    integrity = summary["integrity"]
    lines = [
        "# Generated solver experiment summary",
        "",
        f"Protocol: `{summary['protocol_id']}`. Source revision: `{summary['source_revision']}`.",
        "",
        (
            f"Integrity: {integrity['observed_records']}/{integrity['declared_records']} records, "
            f"{integrity['duplicate_records']} duplicates, {integrity['scenario_fingerprint_mismatches']} "
            "cross-solver scenario mismatches."
        ),
        "",
        "Failed, timed-out, empty, panicking, and verifier-invalid runs remain in every denominator. "
        "Gap and runtime summaries use successful runs only and always show `success/declared`.",
        "",
        "## Overall robustness and runtime",
        "",
    ]
    overall_rows = []
    for row in summary["overall"]:
        overall_rows.append(
            [
                SHORT_LABELS.get(row["solver_id"], row["solver_id"]),
                f"{row['successful']}/{row['declared']}",
                str(row["failed"]),
                str(row["termination_counts"].get("iteration_limit", 0)),
                format_number(row["runtime_median_seconds"], 4),
                format_number(row["lp_gap_median_percent"], 3),
            ]
        )
    lines.append(
        markdown_table(
            ["Solver", "Success", "Failed", "At cap", "Median seconds", "Median LP gap %"],
            overall_rows,
        )
    )

    lines.extend(["", "## Quality suite", ""])
    quality_rows = []
    for row in summary["quality"]:
        quality_rows.append(
            [
                row["profile"],
                SHORT_LABELS.get(row["solver_id"], row["solver_id"]),
                f"{row['successful']}/{row['declared']}",
                format_number(row["lp_gap_median_percent"], 3),
                (
                    f"[{format_number(row['lp_gap_p25_percent'], 3)}, "
                    f"{format_number(row['lp_gap_p75_percent'], 3)}]"
                ),
                format_number(row["allocation_l1_median"], 3),
            ]
        )
    lines.append(
        markdown_table(
            ["Profile", "Solver", "Success", "Median LP gap %", "IQR %", "Median allocation L1"],
            quality_rows,
        )
    )

    lines.extend(["", "## Scaling suite", ""])
    scaling_rows = []
    for row in summary["scaling"]:
        scaling_rows.append(
            [
                row["scale"],
                SHORT_LABELS.get(row["solver_id"], row["solver_id"]),
                f"{row['successful']}/{row['declared']}",
                format_number(row["runtime_median_seconds"], 4),
                (
                    f"[{format_number(row['runtime_p25_seconds'], 4)}, "
                    f"{format_number(row['runtime_p75_seconds'], 4)}]"
                ),
            ]
        )
    lines.append(markdown_table(["Scale", "Solver", "Success", "Median seconds", "IQR seconds"], scaling_rows))

    lines.extend(["", "## Budget sweep", ""])
    budget_rows = []
    for row in summary["budget"]:
        ci = row["lp_gap_mean_ci95_percent"]
        budget_rows.append(
            [
                f"{row['budget_scale']:g}×",
                SHORT_LABELS.get(row["solver_id"], row["solver_id"]),
                f"{row['successful']}/{row['declared']}",
                format_number(row["lp_gap_mean_percent"], 3),
                f"[{format_number(ci[0], 3)}, {format_number(ci[1], 3)}]" if ci else "—",
            ]
        )
    lines.append(markdown_table(["Budget", "Solver", "Success", "Mean LP gap %", "Bootstrap 95% CI"], budget_rows))

    output.write_text("\n".join(lines) + "\n")


def write_csv(summary: dict[str, Any], output: Path) -> None:
    rows = []
    for section in ("overall", "quality", "scaling", "budget", "decomposition", "reference"):
        for row in summary[section]:
            flat = {"section": section, **row}
            flat["status_counts"] = json.dumps(flat["status_counts"], sort_keys=True)
            flat["termination_counts"] = json.dumps(flat["termination_counts"], sort_keys=True)
            flat["lp_gap_mean_ci95_percent"] = json.dumps(flat["lp_gap_mean_ci95_percent"])
            rows.append(flat)
    fields = sorted({key for row in rows for key in row})
    with output.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def svg_document(width: int, height: int, body: list[str]) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}">\n'
        '<rect width="100%" height="100%" fill="white"/>\n'
        '<style>text{font-family:Inter,Arial,sans-serif;fill:#111827}.axis{stroke:#9ca3af;stroke-width:1}'
        '.grid{stroke:#e5e7eb;stroke-width:1}.small{font-size:11px}.tiny{font-size:9px}</style>\n'
        + "\n".join(body)
        + "\n</svg>\n"
    )


def text(x: float, y: float, value: str, size: int = 11, anchor: str = "start", weight: str = "normal") -> str:
    return (
        f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" text-anchor="{anchor}" '
        f'font-weight="{weight}">{html.escape(value)}</text>'
    )


def write_quality_figure(summary: dict[str, Any], output: Path) -> None:
    rows = summary["quality"]
    profiles = sorted({row["profile"] for row in rows})
    solvers = ["lp", "iter-lp", "eg", "conic-quasi", "conic-fisher"]
    successes = [row for row in rows if row["lp_gap_p75_percent"] is not None]
    y_max = max([row["lp_gap_p75_percent"] for row in successes] + [0.1]) * 1.15
    y_min = min([row["lp_gap_p25_percent"] for row in successes] + [0.0])
    if y_max <= y_min:
        y_max = y_min + 1.0
    width, height = 900, 520
    body = [text(450, 24, "Welfare shortfall relative to production LP", 17, "middle", "bold")]
    lookup = {(row["profile"], row["solver_id"]): row for row in rows}
    for panel, profile in enumerate(profiles):
        col, row_index = panel % 2, panel // 2
        x0, y0, plot_w, plot_h = 70 + col * 440, 55 + row_index * 225, 360, 165
        body.append(text(x0, y0 - 12, profile, 13, "start", "bold"))
        body.append(f'<line class="axis" x1="{x0}" y1="{y0}" x2="{x0}" y2="{y0 + plot_h}"/>')
        body.append(f'<line class="axis" x1="{x0}" y1="{y0 + plot_h}" x2="{x0 + plot_w}" y2="{y0 + plot_h}"/>')
        for tick in range(5):
            value = y_min + (y_max - y_min) * tick / 4
            y = y0 + plot_h - plot_h * tick / 4
            body.append(f'<line class="grid" x1="{x0}" y1="{y}" x2="{x0 + plot_w}" y2="{y}"/>')
            body.append(text(x0 - 7, y + 4, f"{value:.2f}", 9, "end"))
        for index, solver in enumerate(solvers):
            item = lookup.get((profile, solver))
            x = x0 + (index + 0.5) * plot_w / len(solvers)
            body.append(text(x, y0 + plot_h + 17, SHORT_LABELS[solver], 9, "middle"))
            if not item or item["lp_gap_median_percent"] is None:
                body.append(text(x, y0 + plot_h / 2, "×", 16, "middle"))
                continue
            transform = lambda value: y0 + plot_h - (value - y_min) / (y_max - y_min) * plot_h
            low, middle, high = (
                transform(item["lp_gap_p25_percent"]),
                transform(item["lp_gap_median_percent"]),
                transform(item["lp_gap_p75_percent"]),
            )
            color = COLORS[solver]
            body.append(f'<line x1="{x}" y1="{high}" x2="{x}" y2="{low}" stroke="{color}" stroke-width="3"/>')
            body.append(f'<circle cx="{x}" cy="{middle}" r="4" fill="{color}"/>')
            if item["failed"]:
                body.append(text(x + 7, y0 + 10, f"×{item['failed']}", 9, "start"))
    body.append(text(16, 270, "LP gap (%)", 11, "middle", "normal"))
    output.write_text(svg_document(width, height, body))


def write_scaling_figure(summary: dict[str, Any], records: list[dict[str, Any]], output: Path) -> None:
    rows = summary["scaling"]
    scale_orders = {
        row["scale"]: row["problem"]["declared_retail_orders"]
        for row in records
        if row["suite"] == "scaling"
    }
    solvers = ["lp", "iter-lp", "eg", "conic-quasi"]
    ordered_scales = sorted(scale_orders, key=scale_orders.get)
    successful = [row for row in rows if row["runtime_median_seconds"] and row["runtime_median_seconds"] > 0]
    y_values = [row["runtime_median_seconds"] for row in successful]
    y_min, y_max = min(y_values) / 1.5, max(y_values) * 1.5
    width, height = 760, 430
    x0, y0, plot_w, plot_h = 85, 50, 620, 300
    body = [text(width / 2, 25, "Solver wall time scaling", 17, "middle", "bold")]
    body += [
        f'<line class="axis" x1="{x0}" y1="{y0}" x2="{x0}" y2="{y0 + plot_h}"/>',
        f'<line class="axis" x1="{x0}" y1="{y0 + plot_h}" x2="{x0 + plot_w}" y2="{y0 + plot_h}"/>',
    ]
    log_min, log_max = math.log10(y_min), math.log10(y_max)
    y_transform = lambda value: y0 + plot_h - (math.log10(value) - log_min) / (log_max - log_min) * plot_h
    for exponent in range(math.floor(log_min), math.ceil(log_max) + 1):
        value = 10**exponent
        if y_min <= value <= y_max:
            y = y_transform(value)
            body.append(f'<line class="grid" x1="{x0}" y1="{y}" x2="{x0 + plot_w}" y2="{y}"/>')
            body.append(text(x0 - 8, y + 4, f"{value:g}s", 10, "end"))
    lookup = {(row["scale"], row["solver_id"]): row for row in rows}
    for index, scale in enumerate(ordered_scales):
        x = x0 + index * plot_w / max(1, len(ordered_scales) - 1)
        body.append(text(x, y0 + plot_h + 20, f"{scale_orders[scale]:,}", 10, "middle"))
    for solver in solvers:
        points = []
        for index, scale in enumerate(ordered_scales):
            item = lookup.get((scale, solver))
            if item and item["runtime_median_seconds"]:
                x = x0 + index * plot_w / max(1, len(ordered_scales) - 1)
                y = y_transform(item["runtime_median_seconds"])
                points.append((x, y))
        color = COLORS[solver]
        if points:
            body.append('<polyline fill="none" stroke="{}" stroke-width="2.5" points="{}"/>'.format(color, " ".join(f"{x:.1f},{y:.1f}" for x, y in points)))
            body.extend(f'<circle cx="{x}" cy="{y}" r="4" fill="{color}"/>' for x, y in points)
    legend_x = x0
    for solver in solvers:
        body.append(f'<rect x="{legend_x}" y="{height - 40}" width="12" height="3" fill="{COLORS[solver]}"/>')
        body.append(text(legend_x + 18, height - 35, SHORT_LABELS[solver], 10))
        legend_x += 125
    body.append(text(x0 + plot_w / 2, height - 57, "Declared retail orders (log-time axis)", 11, "middle"))
    output.write_text(svg_document(width, height, body))


def write_budget_figure(summary: dict[str, Any], output: Path) -> None:
    rows = summary["budget"]
    solvers = ["iter-lp", "eg", "conic-quasi"]
    scales = sorted({float(row["budget_scale"]) for row in rows})
    plotted = [row for row in rows if row["solver_id"] in solvers and row["lp_gap_mean_ci95_percent"]]
    all_bounds = [bound for row in plotted for bound in row["lp_gap_mean_ci95_percent"]]
    y_min, y_max = min(all_bounds + [0.0]), max(all_bounds + [0.1])
    padding = max(0.05, (y_max - y_min) * 0.12)
    y_min, y_max = y_min - padding, y_max + padding
    width, height = 760, 430
    x0, y0, plot_w, plot_h = 80, 50, 620, 300
    body = [text(width / 2, 25, "Mean welfare shortfall across the budget sweep", 17, "middle", "bold")]
    body += [
        f'<line class="axis" x1="{x0}" y1="{y0}" x2="{x0}" y2="{y0 + plot_h}"/>',
        f'<line class="axis" x1="{x0}" y1="{y0 + plot_h}" x2="{x0 + plot_w}" y2="{y0 + plot_h}"/>',
    ]
    y_transform = lambda value: y0 + plot_h - (value - y_min) / (y_max - y_min) * plot_h
    for tick in range(6):
        value = y_min + (y_max - y_min) * tick / 5
        y = y_transform(value)
        body.append(f'<line class="grid" x1="{x0}" y1="{y}" x2="{x0 + plot_w}" y2="{y}"/>')
        body.append(text(x0 - 8, y + 4, f"{value:.2f}%", 10, "end"))
    lookup = {(float(row["budget_scale"]), row["solver_id"]): row for row in rows}
    for index, scale in enumerate(scales):
        x = x0 + index * plot_w / max(1, len(scales) - 1)
        body.append(text(x, y0 + plot_h + 20, f"{scale:g}×", 10, "middle"))
    for solver in solvers:
        points = []
        color = COLORS[solver]
        for index, scale in enumerate(scales):
            item = lookup.get((scale, solver))
            if not item or item["lp_gap_mean_percent"] is None:
                continue
            x = x0 + index * plot_w / max(1, len(scales) - 1)
            middle = y_transform(item["lp_gap_mean_percent"])
            low, high = [y_transform(value) for value in item["lp_gap_mean_ci95_percent"]]
            body.append(f'<line x1="{x}" y1="{high}" x2="{x}" y2="{low}" stroke="{color}" stroke-width="2"/>')
            points.append((x, middle))
        if points:
            body.append('<polyline fill="none" stroke="{}" stroke-width="2.5" points="{}"/>'.format(color, " ".join(f"{x:.1f},{y:.1f}" for x, y in points)))
            body.extend(f'<circle cx="{x}" cy="{y}" r="4" fill="{color}"/>' for x, y in points)
    legend_x = x0
    for solver in solvers:
        body.append(f'<rect x="{legend_x}" y="{height - 40}" width="12" height="3" fill="{COLORS[solver]}"/>')
        body.append(text(legend_x + 18, height - 35, SHORT_LABELS[solver], 10))
        legend_x += 150
    body.append(text(x0 + plot_w / 2, height - 57, "MM budget multiplier; bars are paired bootstrap 95% intervals", 11, "middle"))
    output.write_text(svg_document(width, height, body))


def write_termination_figure(summary: dict[str, Any], output: Path) -> None:
    rows = summary["overall"]
    categories = [
        ("converged", "#16a34a"),
        ("iteration_limit", "#f59e0b"),
        ("delegated", "#60a5fa"),
        ("failed", "#dc2626"),
    ]
    width, height = 780, 80 + 48 * len(rows)
    x0, plot_w = 150, 560
    body = [text(width / 2, 25, "Every declared run by termination outcome", 17, "middle", "bold")]
    for index, row in enumerate(rows):
        y = 55 + index * 48
        total = row["declared"]
        counts = row["termination_counts"]
        failed = row["failed"]
        values = {
            "converged": counts.get("converged", 0),
            "iteration_limit": counts.get("iteration_limit", 0),
            "delegated": counts.get("delegated", 0),
            "failed": failed,
        }
        body.append(text(x0 - 10, y + 14, SHORT_LABELS.get(row["solver_id"], row["solver_id"]), 11, "end"))
        cursor = x0
        for category, color in categories:
            segment = plot_w * values[category] / total
            if segment > 0:
                body.append(f'<rect x="{cursor}" y="{y}" width="{segment}" height="20" fill="{color}"/>')
                if segment > 25:
                    body.append(text(cursor + segment / 2, y + 14, str(values[category]), 9, "middle"))
            cursor += segment
        body.append(text(x0 + plot_w + 8, y + 14, f"n={total}", 10))
    legend_x = x0
    for category, color in categories:
        body.append(f'<rect x="{legend_x}" y="{height - 25}" width="10" height="10" fill="{color}"/>')
        body.append(text(legend_x + 15, height - 16, category.replace("_", " "), 9))
        legend_x += 130
    output.write_text(svg_document(width, height, body))


def main() -> None:
    args = parse_args()
    output_dir = args.output_dir or args.input_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    figures = output_dir / "figures"
    figures.mkdir(exist_ok=True)

    protocol, metadata, records, integrity = load_and_validate(args.input_dir, args.allow_incomplete)
    summary = make_summary(protocol, metadata, records, integrity)
    (output_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    write_csv(summary, output_dir / "summary.csv")
    write_markdown(summary, output_dir / "summary.md")
    write_quality_figure(summary, figures / "quality-welfare-gap.svg")
    write_scaling_figure(summary, records, figures / "scaling-runtime.svg")
    write_budget_figure(summary, figures / "budget-welfare-gap.svg")
    write_termination_figure(summary, figures / "termination-outcomes.svg")
    print(f"validated {len(records)} records; wrote {output_dir}")


if __name__ == "__main__":
    main()
