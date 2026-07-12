"""Compare two explicit arena calibration windows on one shared cohort.

Usage:
    cd arena
    uv run python -m scripts.calibration_compare \
        --before-db before.db --before-since 2026-07-01T00:00:00Z \
        --before-until 2026-07-02T00:00:00Z \
        --after-db after.db --after-since 2026-07-02T00:00:00Z \
        --after-until 2026-07-03T00:00:00Z

The windows are half-open: ``since`` is inclusive and ``until`` is exclusive.
Forecast metrics use only market IDs that have decisions in both windows and a
shared explicit resolved outcome. Inferred outcomes require an explicit
exploratory override. Portfolio PnL remains a whole-account measurement and
compares only exact durable trader identities present in both windows.
"""

from __future__ import annotations

import argparse
import json
import math
import sqlite3
from datetime import datetime
from pathlib import Path
from typing import Any

from scripts.calibration import (
    _connect,
    _parse_iso_timestamp,
    _select_decisions,
    analyze_decisions_db,
    load_outcomes,
)

PROTOCOL_MIN_WINDOW_HOURS = 24.0


def _window_bounds(label: str, since: str | None, until: str | None) -> tuple[datetime, datetime]:
    if since is None or not since.strip() or until is None or not until.strip():
        raise ValueError(f"{label} window requires explicit --{label}-since and --{label}-until")
    since_dt = _parse_iso_timestamp(since)
    until_dt = _parse_iso_timestamp(until)
    if since_dt is None or until_dt is None:  # Defensive: explicit blanks are rejected above.
        raise ValueError(f"{label} window bounds must be non-empty ISO timestamps")
    if since_dt >= until_dt:
        raise ValueError(f"{label} window since must be earlier than until")
    return since_dt, until_dt


def _window_inventory(
    label: str,
    db_path: str,
    since: str | None,
    until: str | None,
    resolved_threshold: float,
) -> tuple[set[int], dict[int, float], str]:
    path = Path(db_path)
    if not path.is_file():
        raise ValueError(f"{label} database does not exist or is not a file: {path}")
    since_dt, until_dt = _window_bounds(label, since, until)
    conn = _connect(str(path))
    try:
        rows = _select_decisions(conn, since_dt, until_dt)
        if not rows:
            raise ValueError(
                f"{label} window contains no decisions: "
                f"[{since_dt.isoformat()}, {until_dt.isoformat()})"
            )
        market_ids = {int(row["market_id"]) for row in rows}
        if not market_ids:
            raise ValueError(f"{label} window contains no market IDs")
        outcomes, source = load_outcomes(conn, resolved_threshold)
        return market_ids, outcomes, source
    finally:
        conn.close()


def _metric(before: int | float | None, after: int | float | None) -> dict[str, Any]:
    return {
        "before": before,
        "after": after,
        "delta": after - before if before is not None and after is not None else None,
    }


def _matched_portfolio_identities(
    before: dict[str, Any], after: dict[str, Any]
) -> dict[str, Any]:
    """Match whole-account PnL only on exact durable names within each arm."""
    result = {}
    for arm in ("flat", "kelly", "native_noise"):
        before_by_name = before.get("portfolio_pnl_by_trader", {}).get(arm, {})
        after_by_name = after.get("portfolio_pnl_by_trader", {}).get(arm, {})
        before_names = set(before_by_name)
        after_names = set(after_by_name)
        matched = sorted(before_names & after_names)
        by_trader = {
            name: _metric(before_by_name[name], after_by_name[name]) for name in matched
        }
        before_values = [before_by_name[name] for name in matched]
        after_values = [after_by_name[name] for name in matched]
        result[arm] = {
            "matched_trader_names": matched,
            "excluded_before_trader_names": sorted(before_names - after_names),
            "excluded_after_trader_names": sorted(after_names - before_names),
            "by_trader": by_trader,
            "matched_mean_pnl": _metric(
                sum(before_values) / len(before_values) if before_values else None,
                sum(after_values) / len(after_values) if after_values else None,
            ),
        }
    if not result["flat"]["matched_trader_names"]:
        raise ValueError(
            "Flat-arm PnL comparison has no matched durable trader identities; "
            "the cohort/account set changed, so use like-for-like experiment windows"
        )
    return result


def _comparison_deltas(
    before: dict[str, Any],
    after: dict[str, Any],
    portfolio_identities: dict[str, Any],
) -> dict[str, Any]:
    before_personas = {row["persona"]: row for row in before["personas"]}
    after_personas = {row["persona"]: row for row in after["personas"]}
    personas = {
        persona: {
            "decision_rows": _metric(
                before_personas.get(persona, {}).get("n"),
                after_personas.get(persona, {}).get("n"),
            ),
            "brier": _metric(
                before_personas.get(persona, {}).get("brier"),
                after_personas.get(persona, {}).get("brier"),
            ),
            "market_price_brier": _metric(
                before_personas.get(persona, {}).get("market_price_brier"),
                after_personas.get(persona, {}).get("market_price_brier"),
            ),
        }
        for persona in sorted(before_personas.keys() | after_personas.keys())
    }
    portfolio_pnl = {
        arm: {
            "mean_pnl": portfolio_identities[arm]["matched_mean_pnl"],
            "by_trader": portfolio_identities[arm]["by_trader"],
        }
        for arm in ("flat", "kelly", "native_noise")
    }
    return {
        "semantics": "delta is after minus before",
        "overall": {
            "decision_rows": _metric(before["overall"]["n"], after["overall"]["n"]),
            "brier": _metric(before["overall"]["brier"], after["overall"]["brier"]),
            "market_price_brier": _metric(
                before["overall"]["market_price_brier"],
                after["overall"]["market_price_brier"],
            ),
        },
        "personas": personas,
        "portfolio_pnl": portfolio_pnl,
    }


def compare_decisions_dbs(
    *,
    before_db: str,
    before_since: str | None,
    before_until: str | None,
    after_db: str,
    after_since: str | None,
    after_until: str | None,
    bins: int = 10,
    resolved_threshold: float = 0.95,
    top_n: int = 10,
    min_window_hours: float = PROTOCOL_MIN_WINDOW_HOURS,
    allow_inferred_outcomes: bool = False,
) -> dict[str, Any]:
    """Analyze two windows on their exact shared, resolved, scoreable cohort."""
    if bins <= 0:
        raise ValueError("bins must be positive")
    if not 0.5 < resolved_threshold <= 1.0:
        raise ValueError("resolved_threshold must be in (0.5, 1.0]")
    if top_n < 0:
        raise ValueError("top_n must be non-negative")

    min_window_hours = float(min_window_hours)
    if not math.isfinite(min_window_hours) or min_window_hours < 0:
        raise ValueError("min_window_hours must be a finite non-negative number")
    before_since_dt, before_until_dt = _window_bounds("before", before_since, before_until)
    after_since_dt, after_until_dt = _window_bounds("after", after_since, after_until)
    before_duration_hours = (before_until_dt - before_since_dt).total_seconds() / 3600
    after_duration_hours = (after_until_dt - after_since_dt).total_seconds() / 3600
    for label, duration_hours in (
        ("before", before_duration_hours),
        ("after", after_duration_hours),
    ):
        if duration_hours < min_window_hours:
            raise ValueError(
                f"{label} window is {duration_hours:g}h, below the configured "
                f"{min_window_hours:g}h minimum; explicitly lower --min-window-hours "
                "only for an exploratory run"
            )

    before_ids, before_outcomes, before_outcome_source = _window_inventory(
        "before", before_db, before_since, before_until, resolved_threshold
    )
    after_ids, after_outcomes, after_outcome_source = _window_inventory(
        "after", after_db, after_since, after_until, resolved_threshold
    )
    inferred_sources = {
        label: source
        for label, source in (
            ("before", before_outcome_source),
            ("after", after_outcome_source),
        )
        if source != "explicit"
    }
    if inferred_sources and not allow_inferred_outcomes:
        details = ", ".join(f"{label}={source}" for label, source in inferred_sources.items())
        raise ValueError(
            f"both databases require explicit outcomes ({details}); copy the same authoritative "
            "market_outcomes labels into working copies of both databases, or pass "
            "--allow-inferred-outcomes only for an exploratory run"
        )
    shared_available = before_ids & after_ids
    if not shared_available:
        raise ValueError("before and after windows have no market IDs in common")

    shared_resolved = shared_available & before_outcomes.keys() & after_outcomes.keys()
    conflicts = sorted(
        market_id
        for market_id in shared_resolved
        if not math.isclose(
            before_outcomes[market_id], after_outcomes[market_id], rel_tol=0.0, abs_tol=1e-12
        )
    )
    if conflicts:
        raise ValueError(f"shared markets have conflicting resolved outcomes: {conflicts}")
    if not shared_resolved:
        raise ValueError("shared market cohort has no resolved outcomes in both databases")

    analysis_args = {
        "bins": bins,
        "resolved_threshold": resolved_threshold,
        "top_n": top_n,
    }
    # First run the existing analyzer on the exact market-ID intersection
    # discovered from the two windows. Its scored IDs then identify the common
    # resolved/usable subset without duplicating forecast-validity rules here.
    before_probe = analyze_decisions_db(
        before_db,
        since=before_since,
        until=before_until,
        market_ids=shared_available,
        **analysis_args,
    )
    after_probe = analyze_decisions_db(
        after_db,
        since=after_since,
        until=after_until,
        market_ids=shared_available,
        **analysis_args,
    )
    shared_scoreable = set(before_probe["cohort"]["scored_market_ids"]) & set(
        after_probe["cohort"]["scored_market_ids"]
    )
    if not shared_scoreable:
        raise ValueError("shared resolved market cohort has no scoreable outcomes in both windows")

    # Re-run on the exact shared scoreable set so every forecast delta uses the
    # same markets, even if one database contained malformed/unusable rows.
    before = analyze_decisions_db(
        before_db,
        since=before_since,
        until=before_until,
        market_ids=shared_scoreable,
        **analysis_args,
    )
    after = analyze_decisions_db(
        after_db,
        since=after_since,
        until=after_until,
        market_ids=shared_scoreable,
        **analysis_args,
    )
    portfolio_identities = _matched_portfolio_identities(before, after)

    return {
        "measurement_protocol": {
            "window_semantics": "since inclusive, until exclusive",
            "protocol_default_min_window_hours": PROTOCOL_MIN_WINDOW_HOURS,
            "configured_min_window_hours": min_window_hours,
            "minimum_window_override": min_window_hours != PROTOCOL_MIN_WINDOW_HOURS,
            "exploratory_minimum_override": min_window_hours < PROTOCOL_MIN_WINDOW_HOURS,
            "before_window_duration_hours": before_duration_hours,
            "after_window_duration_hours": after_duration_hours,
            "before_outcome_source": before_outcome_source,
            "after_outcome_source": after_outcome_source,
            "allow_inferred_outcomes": bool(allow_inferred_outcomes),
            "inferred_outcomes_override_used": bool(inferred_sources)
            and bool(allow_inferred_outcomes),
        },
        "cohort": {
            "before_window_market_ids": sorted(before_ids),
            "after_window_market_ids": sorted(after_ids),
            "shared_available_market_ids": sorted(shared_available),
            "shared_resolved_market_ids": sorted(shared_resolved),
            "shared_scoreable_market_ids": sorted(shared_scoreable),
            "excluded_shared_market_ids": sorted(shared_available - shared_scoreable),
        },
        "before": before,
        "after": after,
        "portfolio_identity_matching": portfolio_identities,
        "deltas": _comparison_deltas(before, after, portfolio_identities),
        "portfolio_pnl_scope": "all_trader_positions",
        "portfolio_pnl_note": (
            "Portfolio PnL is whole-account scope, matched on exact durable trader names, and "
            "is not filtered to the shared market cohort; excluded identities are reported."
        ),
    }


def _fmt(value: Any, precision: int = 4) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.{precision}f}"
    return str(value)


def format_delta_table(result: dict[str, Any]) -> str:
    rows: list[tuple[str, dict[str, Any]]] = [
        ("decision rows", result["deltas"]["overall"]["decision_rows"]),
        ("overall Brier", result["deltas"]["overall"]["brier"]),
        ("market baseline Brier", result["deltas"]["overall"]["market_price_brier"]),
        ("Flat mean PnL", result["deltas"]["portfolio_pnl"]["flat"]["mean_pnl"]),
        ("Kelly mean PnL", result["deltas"]["portfolio_pnl"]["kelly"]["mean_pnl"]),
        ("Native noise mean PnL", result["deltas"]["portfolio_pnl"]["native_noise"]["mean_pnl"]),
    ]
    rows.extend(
        (f"{persona} Brier", metrics["brier"])
        for persona, metrics in result["deltas"]["personas"].items()
    )
    for arm in ("flat", "kelly", "native_noise"):
        rows.extend(
            (f"{arm} {name} PnL", metric)
            for name, metric in result["deltas"]["portfolio_pnl"][arm]["by_trader"].items()
        )
    cohort = result["cohort"]
    identity_matching = result["portfolio_identity_matching"]
    metric_width = max(28, *(len(label) for label, _metric_row in rows))
    lines = [
        "Arena calibration delta (after - before)",
        (
            "Windows: before="
            f"{result['measurement_protocol']['before_window_duration_hours']:g}h, after="
            f"{result['measurement_protocol']['after_window_duration_hours']:g}h, minimum="
            f"{result['measurement_protocol']['configured_min_window_hours']:g}h"
        ),
        (
            "Outcome sources: before="
            f"{result['measurement_protocol']['before_outcome_source']}, after="
            f"{result['measurement_protocol']['after_outcome_source']}, allow inferred="
            f"{str(result['measurement_protocol']['allow_inferred_outcomes']).lower()}, "
            "override used="
            f"{str(result['measurement_protocol']['inferred_outcomes_override_used']).lower()}"
        ),
        "Shared available market IDs: "
        + ",".join(str(value) for value in cohort["shared_available_market_ids"]),
        "Shared scoreable market IDs: "
        + ",".join(str(value) for value in cohort["shared_scoreable_market_ids"]),
        "Matched Flat trader identities: "
        + ",".join(identity_matching["flat"]["matched_trader_names"]),
        "Excluded before Flat identities: "
        + ",".join(identity_matching["flat"]["excluded_before_trader_names"]),
        "Excluded after Flat identities: "
        + ",".join(identity_matching["flat"]["excluded_after_trader_names"]),
        result["portfolio_pnl_note"],
        "",
        f"{'metric':{metric_width}s} {'before':>12s} {'after':>12s} {'delta':>12s}",
        "-" * (metric_width + 39),
    ]
    lines.extend(
        f"{label:{metric_width}s} {_fmt(metric['before']):>12s} "
        f"{_fmt(metric['after']):>12s} {_fmt(metric['delta']):>12s}"
        for label, metric in rows
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Compare two explicit arena calibration windows on a shared market cohort"
    )
    parser.add_argument("--before-db", required=True, help="Path to the before decisions DB")
    parser.add_argument("--before-since", required=True, help="Inclusive before-window start")
    parser.add_argument("--before-until", required=True, help="Exclusive before-window end")
    parser.add_argument("--after-db", required=True, help="Path to the after decisions DB")
    parser.add_argument("--after-since", required=True, help="Inclusive after-window start")
    parser.add_argument("--after-until", required=True, help="Exclusive after-window end")
    parser.add_argument("--bins", type=int, default=10, help="Reliability curve bin count")
    parser.add_argument(
        "--resolved-threshold",
        type=float,
        default=0.95,
        help="Infer outcomes from final prices only outside this threshold",
    )
    parser.add_argument(
        "--top-n", type=int, default=10, help="Number of submitted-order surprises per report"
    )
    parser.add_argument(
        "--min-window-hours",
        type=float,
        default=PROTOCOL_MIN_WINDOW_HOURS,
        help=(
            "Required duration for each window (protocol default: 24); lower explicitly only "
            "for exploratory runs"
        ),
    )
    parser.add_argument(
        "--allow-inferred-outcomes",
        action="store_true",
        help="Allow final-price-inferred outcomes for an exploratory, non-authoritative run",
    )
    parser.add_argument("--json-out", default="", help="Optional path to write JSON output")
    args = parser.parse_args()

    try:
        result = compare_decisions_dbs(
            before_db=args.before_db,
            before_since=args.before_since,
            before_until=args.before_until,
            after_db=args.after_db,
            after_since=args.after_since,
            after_until=args.after_until,
            bins=args.bins,
            resolved_threshold=args.resolved_threshold,
            top_n=args.top_n,
            min_window_hours=args.min_window_hours,
            allow_inferred_outcomes=args.allow_inferred_outcomes,
        )
    except (OSError, sqlite3.Error, ValueError) as exc:
        parser.error(str(exc))
    print(format_delta_table(result))
    json_text = json.dumps(result, indent=2, sort_keys=True)
    print("\nJSON:")
    print(json_text)
    if args.json_out:
        Path(args.json_out).write_text(json_text + "\n")


if __name__ == "__main__":
    main()
