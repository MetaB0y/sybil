"""Offline calibration report for live arena decisions.

Usage:
    cd arena
    uv run python -m scripts.calibration --db live/decisions.db
    uv run python -m scripts.calibration --db live/decisions.db --json-out calibration.json
    uv run python -m scripts.calibration --db live/decisions.db --market-ids 1,2,3
"""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import math
import re
import sqlite3
import statistics
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from live.personas import PERSONAS


STAGE1_AB_MODE = "syb-114-stage1-ab"
MIN_EXPERIMENT_WINDOW_SECONDS = 24 * 60 * 60
PORTFOLIO_SNAPSHOT_CADENCE_SECONDS = 5 * 60
PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS = 10 * 60


def _connect(db_path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    return conn


def _has_table(conn: sqlite3.Connection, table: str) -> bool:
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?",
        (table,),
    ).fetchone()
    return row is not None


def _columns(conn: sqlite3.Connection, table: str) -> set[str]:
    if not _has_table(conn, table):
        return set()
    return {row["name"] for row in conn.execute(f"PRAGMA table_info({table})")}


def _safe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if math.isnan(number) or math.isinf(number):
        return None
    return number


def _clamp_probability(value: Any) -> float | None:
    number = _safe_float(value)
    if number is None:
        return None
    return min(1.0, max(0.0, number))


def _outcome_from_value(value: Any) -> float | None:
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"yes", "true", "1", "resolved_yes"}:
            return 1.0
        if normalized in {"no", "false", "0", "resolved_no"}:
            return 0.0
    number = _safe_float(value)
    if number is None:
        return None
    if 0.0 <= number <= 1.0:
        return number
    return None


def _load_explicit_outcomes(conn: sqlite3.Connection) -> dict[int, float]:
    """Load fixture/live outcome overrides from common lightweight schemas."""
    for table in ("market_outcomes", "outcomes", "resolved_markets"):
        cols = _columns(conn, table)
        if "market_id" not in cols:
            continue
        value_col = next(
            (
                col
                for col in (
                    "outcome",
                    "resolved_yes",
                    "yes_outcome",
                    "yes_payout",
                    "payout",
                    "resolution",
                )
                if col in cols
            ),
            None,
        )
        if value_col is None:
            continue
        rows = conn.execute(f"SELECT market_id, {value_col} AS outcome FROM {table}")
        outcomes = {
            int(row["market_id"]): outcome
            for row in rows
            if (outcome := _outcome_from_value(row["outcome"])) is not None
        }
        if outcomes:
            return outcomes
    return {}


def _infer_outcomes_from_final_prices(
    conn: sqlite3.Connection,
    resolved_threshold: float,
) -> dict[int, float]:
    if not _has_table(conn, "decisions"):
        return {}
    rows = conn.execute(
        "SELECT market_id, market_price FROM decisions WHERE market_price IS NOT NULL ORDER BY id"
    )
    latest: dict[int, float] = {}
    for row in rows:
        price = _clamp_probability(row["market_price"])
        if price is not None:
            latest[int(row["market_id"])] = price
    return {
        market_id: 1.0 if price >= resolved_threshold else 0.0
        for market_id, price in latest.items()
        if price >= resolved_threshold or price <= 1.0 - resolved_threshold
    }


def load_outcomes(
    conn: sqlite3.Connection,
    resolved_threshold: float = 0.95,
) -> tuple[dict[int, float], str]:
    explicit = _load_explicit_outcomes(conn)
    if explicit:
        return explicit, "explicit"
    return _infer_outcomes_from_final_prices(conn, resolved_threshold), "final_price_inferred"


def _parse_iso_timestamp(value: str | None) -> datetime | None:
    if value is None:
        return None
    normalized = value.strip()
    if not normalized:
        return None
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise ValueError(f"invalid ISO timestamp: {value!r}") from exc
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _parse_market_ids(value: str | None) -> frozenset[int] | None:
    if value is None or not value.strip():
        return None
    try:
        market_ids = frozenset(int(part.strip()) for part in value.split(",") if part.strip())
    except ValueError as exc:
        raise ValueError("market IDs must be comma-separated integers") from exc
    if not market_ids:
        return None
    if any(market_id < 0 for market_id in market_ids):
        raise ValueError("market IDs must be non-negative")
    return market_ids


def _select_decisions(
    conn: sqlite3.Connection,
    since: datetime | None = None,
    until: datetime | None = None,
) -> list[sqlite3.Row]:
    cols = _columns(conn, "decisions")
    if not cols:
        return []
    optional = [
        "raw_fair_value",
        "effective_fair_value",
        "fair_value_age_s",
        "confidence",
        "countercase",
        "rejection_reason",
        "market_category",
        "market_tags",
        "analysis_batch_id",
        "analysis_reference_price",
    ]
    selected = [
        "id",
        "trader_name",
        "market_id",
        "market_name",
        "timestamp",
        "fair_value",
        "market_price",
        "orders",
        *[col for col in optional if col in cols],
    ]
    rows = list(conn.execute(f"SELECT {', '.join(selected)} FROM decisions ORDER BY id"))
    if since is None and until is None:
        return rows

    filtered = []
    for row in rows:
        timestamp = _parse_iso_timestamp(row["timestamp"])
        if timestamp is None:
            continue
        if since is not None and timestamp < since:
            continue
        # Experiment windows are half-open: [since, until).
        if until is not None and timestamp >= until:
            continue
        filtered.append(row)
    return filtered


def _persona_name(trader_name: str) -> str:
    return re.sub(r"\s+\((Kelly|Flat|Analyst)\)$", "", trader_name).strip()


def _orders_count(raw_orders: Any) -> int:
    if not raw_orders:
        return 0
    try:
        orders = json.loads(raw_orders)
    except (TypeError, json.JSONDecodeError):
        return 0
    return len(orders) if isinstance(orders, list) else 0


def _json_string_list(raw_value: Any) -> list[str]:
    if not raw_value:
        return []
    try:
        values = json.loads(raw_value) if isinstance(raw_value, str) else raw_value
    except json.JSONDecodeError:
        return []
    if not isinstance(values, list):
        return []
    return [str(value) for value in values if str(value).strip()]


def _forecast(row: sqlite3.Row) -> float | None:
    keys = set(row.keys())
    if "effective_fair_value" in keys:
        effective = _clamp_probability(row["effective_fair_value"])
        if effective is not None:
            return effective
    return _clamp_probability(row["fair_value"])


def _raw_forecast(row: sqlite3.Row) -> float | None:
    keys = set(row.keys())
    if "raw_fair_value" in keys:
        raw = _clamp_probability(row["raw_fair_value"])
        if raw is not None:
            return raw
    return _clamp_probability(row["fair_value"])


def _brier(pairs: list[tuple[float, float]]) -> float | None:
    if not pairs:
        return None
    return sum((forecast - outcome) ** 2 for forecast, outcome in pairs) / len(pairs)


def _mean(values: list[float]) -> float | None:
    return sum(values) / len(values) if values else None


def _reliability_curve(
    pairs: list[tuple[float, float]],
    bins: int,
) -> list[dict[str, float | int | None]]:
    buckets: list[list[tuple[float, float]]] = [[] for _ in range(bins)]
    for forecast, outcome in pairs:
        idx = min(bins - 1, max(0, int(forecast * bins)))
        buckets[idx].append((forecast, outcome))

    curve = []
    for idx, bucket in enumerate(buckets):
        forecasts = [forecast for forecast, _outcome in bucket]
        outcomes = [outcome for _forecast, outcome in bucket]
        curve.append(
            {
                "bin": idx,
                "low": idx / bins,
                "high": (idx + 1) / bins,
                "n": len(bucket),
                "mean_forecast": _mean(forecasts),
                "empirical_yes_rate": _mean(outcomes),
                "brier": _brier(bucket),
            }
        )
    return curve


def _rejection_calibration(rows: list[dict[str, Any]]) -> dict[str, Any]:
    acted = [row for row in rows if row["orders_count"] > 0]
    rejected = [row for row in rows if row["orders_count"] == 0]

    def pairs(subset: list[dict[str, Any]]) -> list[tuple[float, float]]:
        return [(row["forecast"], row["outcome"]) for row in subset]

    def edges(subset: list[dict[str, Any]]) -> list[float]:
        return [
            abs(row["forecast"] - row["market_price"])
            for row in subset
            if row["market_price"] is not None
        ]

    def confidences(subset: list[dict[str, Any]]) -> list[float]:
        return [row["confidence"] for row in subset if row["confidence"] is not None]

    per_reason: dict[str, dict[str, Any]] = {}
    for row in rejected:
        reason = row["rejection_reason"] or "unknown"
        stats = per_reason.setdefault(reason, {"n": 0, "would_have_profited_n": 0})
        stats["n"] += 1
        forecast = row["forecast"]
        market = row["market_price"]
        outcome = row["outcome"]
        would_have_profited = (forecast > market and outcome > market) or (
            forecast < market and outcome < market
        )
        if would_have_profited:
            stats["would_have_profited_n"] += 1
    for stats in per_reason.values():
        stats["would_have_lost_or_broken_even_n"] = stats["n"] - stats["would_have_profited_n"]
        stats["would_have_profited_rate"] = stats["would_have_profited_n"] / stats["n"]

    total = len(rows)
    return {
        "definition": "rejected means decision row had no submitted orders",
        "n": total,
        "acted_n": len(acted),
        "rejected_n": len(rejected),
        "coverage": len(acted) / total if total else None,
        "rejection_rate": len(rejected) / total if total else None,
        "acted_brier": _brier(pairs(acted)),
        "rejected_brier": _brier(pairs(rejected)),
        "acted_mean_edge": _mean(edges(acted)),
        "rejected_mean_edge": _mean(edges(rejected)),
        "acted_mean_confidence": _mean(confidences(acted)),
        "rejected_mean_confidence": _mean(confidences(rejected)),
        "by_reason": dict(sorted(per_reason.items())),
    }


def _category_labels(row: dict[str, Any]) -> list[str]:
    category = str(row.get("market_category") or "").strip()
    if category:
        return [category]
    return [str(tag).strip() for tag in row.get("market_tags", []) if str(tag).strip()]


def _by_category_brier(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[str, list[tuple[float, float]]] = {}
    for row in rows:
        for label in _category_labels(row):
            grouped.setdefault(label, []).append((row["forecast"], row["outcome"]))
    return [
        {"category": category, "n": len(pairs), "brier": _brier(pairs)}
        for category, pairs in sorted(grouped.items())
    ]


def _surprises(rows: list[dict[str, Any]], top_n: int) -> list[dict[str, Any]]:
    acted = [row for row in rows if row["orders_count"] > 0]
    acted.sort(key=lambda row: (-abs(row["forecast"] - row["outcome"]), row["id"]))
    return [
        {
            "decision_id": row["id"],
            "analysis_batch_id": row["analysis_batch_id"],
            "analysis_reference_price": row.get("analysis_reference_price"),
            "analysis_market_price": row.get("analysis_market_price"),
            "persona": row["persona"],
            "trader_name": row["trader_name"],
            "market_id": row["market_id"],
            "market_name": row["market_name"],
            "timestamp": row["timestamp"],
            "forecast": row["forecast"],
            "outcome": row["outcome"],
            "absolute_error": abs(row["forecast"] - row["outcome"]),
        }
        for row in acted[:top_n]
    ]


def _deduplicate_analysis_batches(
    rows: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], int]:
    """Keep the first forecast per durable trader, market, and analysis batch."""
    unique = []
    seen = set()
    for row in rows:
        key = (row["trader_name"], row["market_id"], row["analysis_batch_id"])
        if key in seen:
            continue
        seen.add(key)
        unique.append(row)
    return unique, len(rows) - len(unique)


_AB_PERSONA_RE = re.compile(
    r"^(?P<persona>.+) \[SYB-114:(?P<experiment>[^:\]]+):"
    r"(?P<variant>control|stage1)\]$"
)


def _load_stage1_experiment(
    conn: sqlite3.Connection,
    experiment_id: str,
    until: datetime | None,
    exploratory_short_window: bool,
    now: datetime,
) -> dict[str, Any]:
    """Load one immutable Stage-1 record and derive its exact report scope."""
    if not experiment_id or experiment_id != experiment_id.strip():
        raise ValueError("experiment ID must be nonempty without surrounding whitespace")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}", experiment_id):
        raise ValueError("experiment ID must use 1-64 letters, numbers, '.', '_' or '-'")
    if not _has_table(conn, "live_experiments"):
        raise ValueError("database has no live_experiments table")
    row = conn.execute(
        """SELECT experiment_id, mode, started_at_utc, configuration_json
           FROM live_experiments WHERE experiment_id = ?""",
        (experiment_id,),
    ).fetchone()
    if row is None:
        raise ValueError(f"experiment {experiment_id!r} was not found")
    if str(row["mode"]) != STAGE1_AB_MODE:
        raise ValueError(
            f"experiment {experiment_id!r} has mode {row['mode']!r}, expected {STAGE1_AB_MODE!r}"
        )
    if until is None:
        raise ValueError("--experiment-id requires an explicit exclusive --until")
    if until > now:
        raise ValueError("experiment --until cannot be in the future")

    started_at = _parse_iso_timestamp(str(row["started_at_utc"]))
    if started_at is None:
        raise ValueError(f"experiment {experiment_id!r} has no valid started_at_utc")
    if until <= started_at:
        raise ValueError("experiment --until must be later than its persisted start")
    duration_seconds = (until - started_at).total_seconds()
    if duration_seconds < MIN_EXPERIMENT_WINDOW_SECONDS and not exploratory_short_window:
        raise ValueError(
            "strict experiment reports require at least 24 hours; "
            "use --exploratory-short-window to record an explicitly exploratory override"
        )

    try:
        configuration = json.loads(str(row["configuration_json"]))
    except json.JSONDecodeError as exc:
        raise ValueError(f"experiment {experiment_id!r} has invalid configuration JSON") from exc
    if not isinstance(configuration, dict):
        raise ValueError(f"experiment {experiment_id!r} configuration must be an object")

    raw_market_ids = configuration.get("market_ids")
    if not isinstance(raw_market_ids, list) or not raw_market_ids:
        raise ValueError(f"experiment {experiment_id!r} has no frozen market cohort")
    try:
        market_ids = frozenset(int(market_id) for market_id in raw_market_ids)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"experiment {experiment_id!r} has invalid market IDs") from exc
    if len(market_ids) != len(raw_market_ids) or any(market_id < 0 for market_id in market_ids):
        raise ValueError(f"experiment {experiment_id!r} has invalid or duplicate market IDs")

    persona_keys = configuration.get("personas")
    display_hashes = configuration.get("persona_display_name_sha256")
    variants = configuration.get("variants")
    if (
        not isinstance(persona_keys, list)
        or not persona_keys
        or not isinstance(display_hashes, dict)
        or not isinstance(variants, list)
    ):
        raise ValueError(f"experiment {experiment_id!r} lacks immutable identity metadata")
    if not all(isinstance(persona_key, str) for persona_key in persona_keys):
        raise ValueError(f"experiment {experiment_id!r} has invalid persona identities")
    if len(set(persona_keys)) != len(persona_keys):
        raise ValueError(f"experiment {experiment_id!r} has duplicate persona identities")
    variant_ids = [variant.get("id") for variant in variants if isinstance(variant, dict)]
    if variant_ids != ["control", "stage1"]:
        raise ValueError(
            f"experiment {experiment_id!r} does not have the expected control/Stage-1 variants"
        )

    arms = []
    for persona_key in persona_keys:
        if not isinstance(persona_key, str) or persona_key not in PERSONAS:
            raise ValueError(
                f"cannot reconstruct persisted persona identity {persona_key!r} from current code"
            )
        display_name = str(PERSONAS[persona_key]["name"])
        actual_hash = sha256(display_name.encode("utf-8")).hexdigest()
        if display_hashes.get(persona_key) != actual_hash:
            raise ValueError(
                f"persisted display-name fingerprint drifted for persona {persona_key!r}; "
                "refusing to guess durable identities"
            )
        for variant in variant_ids:
            prefix = f"{display_name} [SYB-114:{experiment_id}:{variant}]"
            arms.append(
                {
                    "persona_key": persona_key,
                    "persona": display_name,
                    "variant": variant,
                    "analyst_name": f"{prefix} (Analyst)",
                    "flat_trader_name": f"{prefix} (Flat)",
                }
            )

    return {
        "experiment_id": experiment_id,
        "mode": STAGE1_AB_MODE,
        "started_at_utc": started_at,
        "until_utc": until,
        "duration_seconds": duration_seconds,
        "exploratory_short_window_override": exploratory_short_window,
        "configuration": configuration,
        "market_ids": market_ids,
        "arms": arms,
    }


def _analysis_batch_matching(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Score control/Stage-1 only on their exact shared evidence batches."""
    groups: dict[tuple[str, str], dict[str, dict[str, dict[str, Any]]]] = {}
    for row in rows:
        match = _AB_PERSONA_RE.fullmatch(row["persona"])
        if match is None:
            continue
        key = (match.group("experiment"), match.group("persona"))
        variants = groups.setdefault(key, {"control": {}, "stage1": {}})
        variants[match.group("variant")][row["analysis_batch_id"]] = row

    result = []
    for (experiment_id, persona), variants in sorted(groups.items()):
        control = variants["control"]
        stage1 = variants["stage1"]
        matched_ids = sorted(control.keys() & stage1.keys())

        def metrics(variant_rows: dict[str, dict[str, Any]]) -> dict[str, Any]:
            matched_rows = [variant_rows[batch_id] for batch_id in matched_ids]
            return {
                "n": len(matched_rows),
                "brier": _brier([(row["forecast"], row["outcome"]) for row in matched_rows]),
                "market_price_brier": _brier(
                    [(row["analysis_market_price"], row["outcome"]) for row in matched_rows]
                ),
                "analysis_market_prices": [row["analysis_market_price"] for row in matched_rows],
            }

        control_metrics = metrics(control)
        stage1_metrics = metrics(stage1)
        comparable = bool(matched_ids)
        result.append(
            {
                "experiment_id": experiment_id,
                "persona": persona,
                "comparison_semantics": "Stage1 minus control on exact analysis_batch_id intersection",
                "comparable": comparable,
                "not_comparable_reason": (None if comparable else "no matched analysis batches"),
                "matched_count": len(matched_ids),
                "unmatched_control_count": len(control.keys() - stage1.keys()),
                "unmatched_stage1_count": len(stage1.keys() - control.keys()),
                "control": control_metrics,
                "stage1": stage1_metrics,
                "stage1_minus_control": {
                    "brier": (
                        stage1_metrics["brier"] - control_metrics["brier"] if comparable else None
                    ),
                    "market_price_brier": (
                        stage1_metrics["market_price_brier"] - control_metrics["market_price_brier"]
                        if comparable
                        else None
                    ),
                },
            }
        )
    return result


def _portfolio_pnl_by_trader(
    conn: sqlite3.Connection,
    trader_kind: str,
    since: datetime | None,
    until: datetime | None,
    allowed_trader_names: frozenset[str] | None = None,
) -> dict[str, float]:
    if not _has_table(conn, "portfolio_snapshots"):
        return {}

    def matches(trader_name: str) -> bool:
        if allowed_trader_names is not None and trader_name not in allowed_trader_names:
            return False
        if trader_kind == "flat":
            return trader_name.endswith(" (Flat)")
        if trader_kind == "kelly":
            return trader_name.endswith(" (Kelly)")
        if trader_kind == "native_noise":
            return trader_name.startswith("Noise") or "NativeNoiseTrader" in trader_name
        raise ValueError(f"unknown trader kind: {trader_kind}")

    snapshots: dict[str, list[tuple[datetime, float]]] = {}
    rows = conn.execute("SELECT trader_name, timestamp, pnl FROM portfolio_snapshots ORDER BY id")
    for row in rows:
        trader_name = str(row["trader_name"] or "")
        timestamp = _parse_iso_timestamp(row["timestamp"])
        pnl = _safe_float(row["pnl"])
        if not matches(trader_name) or timestamp is None or pnl is None:
            continue
        snapshots.setdefault(trader_name, []).append((timestamp, pnl))

    pnls: dict[str, float] = {}
    for trader_name, trader_snapshots in snapshots.items():
        trader_snapshots.sort(key=lambda item: item[0])
        eligible = [
            (timestamp, pnl)
            for timestamp, pnl in trader_snapshots
            if until is None or timestamp < until
        ]
        if not eligible:
            continue
        if since is None:
            pnls[trader_name] = eligible[-1][1]
            continue

        in_window = [item for item in eligible if item[0] >= since]
        if not in_window:
            continue
        before_window = [item for item in eligible if item[0] < since]
        starting_pnl = before_window[-1][1] if before_window else in_window[0][1]
        pnls[trader_name] = in_window[-1][1] - starting_pnl

    return dict(sorted(pnls.items()))


def _portfolio_pnl_summary(
    conn: sqlite3.Connection,
    trader_kind: str,
    since: datetime | None,
    until: datetime | None,
    allowed_trader_names: frozenset[str] | None = None,
) -> dict[str, Any]:
    by_trader = _portfolio_pnl_by_trader(conn, trader_kind, since, until, allowed_trader_names)
    pnls = list(by_trader.values())

    if not pnls:
        return {"n": 0}
    return {
        "n": len(pnls),
        "mode": "window_delta" if since is not None else "cumulative",
        "mean_pnl": statistics.fmean(pnls),
        "median_pnl": statistics.median(pnls),
        "min_pnl": min(pnls),
        "max_pnl": max(pnls),
    }


def _stage1_experiment_metrics(
    conn: sqlite3.Connection,
    experiment: dict[str, Any],
    decision_rows: list[sqlite3.Row],
) -> dict[str, Any]:
    """Measure spend, batches, and PnL only for immutable experiment identities."""
    if not _has_table(conn, "token_usage"):
        raise ValueError("strict experiment report requires the token_usage table")

    started_at = experiment["started_at_utc"]
    until = experiment["until_utc"]
    token_columns = _columns(conn, "token_usage")
    required_token_columns = {
        "id",
        "trader_name",
        "timestamp",
        "prompt_tokens",
        "completion_tokens",
        "usd_cost",
    }
    if not required_token_columns <= token_columns:
        missing = ", ".join(sorted(required_token_columns - token_columns))
        raise ValueError(f"strict experiment report token_usage is missing: {missing}")

    snapshot_times: dict[str, list[datetime]] = {
        arm["flat_trader_name"]: [] for arm in experiment["arms"]
    }
    if _has_table(conn, "portfolio_snapshots"):
        for row in conn.execute(
            "SELECT trader_name, timestamp FROM portfolio_snapshots ORDER BY id"
        ):
            trader_name = str(row["trader_name"] or "")
            if trader_name not in snapshot_times:
                continue
            timestamp = _parse_iso_timestamp(row["timestamp"])
            if timestamp is not None and started_at <= timestamp < until:
                snapshot_times[trader_name].append(timestamp)

    coverage_arms = []
    for arm in experiment["arms"]:
        trader_name = arm["flat_trader_name"]
        timestamps = sorted(snapshot_times[trader_name])
        first = timestamps[0] if timestamps else None
        latest = timestamps[-1] if timestamps else None
        start_delay = (first - started_at).total_seconds() if first is not None else None
        end_lag = (until - latest).total_seconds() if latest is not None else None
        gaps = [(right - left).total_seconds() for left, right in zip(timestamps, timestamps[1:])]
        max_gap = max(gaps) if gaps else None
        reasons = []
        if not timestamps:
            reasons.append("no in-window portfolio snapshots")
        else:
            if start_delay > PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS:
                reasons.append(f"first snapshot is {start_delay:.0f}s after start")
            if end_lag > PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS:
                reasons.append(f"latest snapshot is {end_lag:.0f}s before end")
            if max_gap is not None and max_gap > PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS:
                reasons.append(f"maximum consecutive snapshot gap is {max_gap:.0f}s")
        coverage_arms.append(
            {
                "persona_key": arm["persona_key"],
                "persona": arm["persona"],
                "variant": arm["variant"],
                "flat_trader_name": trader_name,
                "snapshot_count": len(timestamps),
                "first_snapshot_utc": first.isoformat() if first is not None else None,
                "latest_snapshot_utc": latest.isoformat() if latest is not None else None,
                "start_delay_seconds": start_delay,
                "end_lag_seconds": end_lag,
                "max_consecutive_gap_seconds": max_gap,
                "coverage_complete": not reasons,
                "incomplete_reasons": reasons,
            }
        )

    snapshot_coverage = {
        "expected_cadence_seconds": PORTFOLIO_SNAPSHOT_CADENCE_SECONDS,
        "maximum_allowed_gap_seconds": PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS,
        "endpoint_tolerance_seconds": PORTFOLIO_SNAPSHOT_COVERAGE_TOLERANCE_SECONDS,
        "coverage_complete": all(arm["coverage_complete"] for arm in coverage_arms),
        "arms": coverage_arms,
    }
    if (
        not snapshot_coverage["coverage_complete"]
        and not experiment["exploratory_short_window_override"]
    ):
        details = "; ".join(
            f"{arm['flat_trader_name']}: {', '.join(arm['incomplete_reasons'])}"
            for arm in coverage_arms
            if not arm["coverage_complete"]
        )
        raise ValueError(f"window coverage incomplete: {details}")

    decision_batches: dict[str, set[tuple[int, str]]] = {
        arm["flat_trader_name"]: set() for arm in experiment["arms"]
    }
    decision_row_counts = {name: 0 for name in decision_batches}
    for row in decision_rows:
        trader_name = str(row["trader_name"] or "")
        if trader_name not in decision_batches:
            continue
        decision_row_counts[trader_name] += 1
        batch_id = (
            str(row["analysis_batch_id"] or "").strip() if "analysis_batch_id" in row.keys() else ""
        )
        if batch_id:
            decision_batches[trader_name].add((int(row["market_id"]), batch_id))

    token_stats = {
        arm["analyst_name"]: {
            "calls": 0,
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "usd": 0.0,
        }
        for arm in experiment["arms"]
    }
    for row in conn.execute(
        """SELECT trader_name, timestamp, prompt_tokens, completion_tokens, usd_cost
           FROM token_usage ORDER BY id"""
    ):
        trader_name = str(row["trader_name"] or "")
        if trader_name not in token_stats:
            continue
        timestamp = _parse_iso_timestamp(row["timestamp"])
        if timestamp is None or timestamp < started_at or timestamp >= until:
            continue
        stats = token_stats[trader_name]
        stats["calls"] += 1
        stats["prompt_tokens"] += int(row["prompt_tokens"] or 0)
        stats["completion_tokens"] += int(row["completion_tokens"] or 0)
        stats["usd"] += _safe_float(row["usd_cost"]) or 0.0

    flat_names = frozenset(arm["flat_trader_name"] for arm in experiment["arms"])
    pnl_by_trader = _portfolio_pnl_by_trader(conn, "flat", started_at, until, flat_names)
    arms = []
    for arm in experiment["arms"]:
        analyst_stats = token_stats[arm["analyst_name"]]
        flat_name = arm["flat_trader_name"]
        decision_count = decision_row_counts[flat_name]
        batch_count = len(decision_batches[flat_name])
        spend = analyst_stats["usd"]
        arms.append(
            {
                **arm,
                **analyst_stats,
                "decision_rows": decision_count,
                "analysis_batch_count": batch_count,
                "usd_per_decision": spend / decision_count if decision_count else None,
                "usd_per_analysis_batch": spend / batch_count if batch_count else None,
                "pnl": pnl_by_trader.get(flat_name),
            }
        )

    comparisons = []
    for persona_key in experiment["configuration"]["personas"]:
        persona_arms = {arm["variant"]: arm for arm in arms if arm["persona_key"] == persona_key}
        control = persona_arms["control"]
        stage1 = persona_arms["stage1"]
        control_batches = decision_batches[control["flat_trader_name"]]
        stage1_batches = decision_batches[stage1["flat_trader_name"]]
        matched = control_batches & stage1_batches

        def delta(field: str) -> float | int | None:
            control_value = control[field]
            stage1_value = stage1[field]
            if control_value is None or stage1_value is None:
                return None
            return stage1_value - control_value

        comparisons.append(
            {
                "persona_key": persona_key,
                "persona": control["persona"],
                "matched_analysis_batch_count": len(matched),
                "unmatched_control_batch_count": len(control_batches - stage1_batches),
                "unmatched_stage1_batch_count": len(stage1_batches - control_batches),
                "control": control,
                "stage1": stage1,
                "stage1_minus_control": {
                    "calls": delta("calls"),
                    "usd": delta("usd"),
                    "usd_per_decision": delta("usd_per_decision"),
                    "usd_per_analysis_batch": delta("usd_per_analysis_batch"),
                    "pnl": delta("pnl"),
                },
            }
        )

    return {
        "identity_semantics": "exact names reconstructed from immutable persona fingerprints",
        "snapshot_coverage": snapshot_coverage,
        "arms": arms,
        "comparisons": comparisons,
        "flat_pnl_by_durable_identity": pnl_by_trader,
    }


def analyze_decisions_db(
    db_path: str,
    bins: int = 10,
    resolved_threshold: float = 0.95,
    since: str | None = None,
    until: str | None = None,
    top_n: int = 10,
    market_ids: frozenset[int] | set[int] | None = None,
    experiment_id: str | None = None,
    exploratory_short_window: bool = False,
    now: datetime | None = None,
) -> dict[str, Any]:
    if bins <= 0:
        raise ValueError("bins must be positive")
    if not 0.5 < resolved_threshold <= 1.0:
        raise ValueError("resolved_threshold must be in (0.5, 1.0]")
    if top_n < 0:
        raise ValueError("top_n must be non-negative")
    since_dt = _parse_iso_timestamp(since)
    until_dt = _parse_iso_timestamp(until)
    cohort = frozenset(int(market_id) for market_id in market_ids) if market_ids else None
    if cohort is not None and any(market_id < 0 for market_id in cohort):
        raise ValueError("market IDs must be non-negative")
    if since_dt is not None and until_dt is not None and since_dt >= until_dt:
        raise ValueError("since must be earlier than until")
    if experiment_id is None and exploratory_short_window:
        raise ValueError("--exploratory-short-window requires --experiment-id")
    if experiment_id is not None and since is not None:
        raise ValueError("--experiment-id derives --since; do not also pass --since")
    if experiment_id is not None and market_ids is not None:
        raise ValueError("--experiment-id derives the frozen cohort; do not pass --market-ids")
    now_dt = now or datetime.now(timezone.utc)
    if now_dt.tzinfo is None:
        now_dt = now_dt.replace(tzinfo=timezone.utc)
    else:
        now_dt = now_dt.astimezone(timezone.utc)

    conn = _connect(db_path)
    try:
        experiment = None
        allowed_flat_names = None
        if experiment_id is not None:
            experiment = _load_stage1_experiment(
                conn,
                experiment_id,
                until_dt,
                exploratory_short_window,
                now_dt,
            )
            since_dt = experiment["started_at_utc"]
            cohort = experiment["market_ids"]
            allowed_flat_names = frozenset(arm["flat_trader_name"] for arm in experiment["arms"])

        if experiment is not None:
            # A strict experiment report must not derive labels from arbitrary
            # decision rows elsewhere in the shared live database.
            outcomes = {
                market_id: outcome
                for market_id, outcome in _load_explicit_outcomes(conn).items()
                if market_id in cohort
            }
            outcome_source = "explicit" if outcomes else "explicit_unavailable"
        else:
            outcomes, outcome_source = load_outcomes(conn, resolved_threshold)
        selected_decisions = [
            row
            for row in _select_decisions(conn, since_dt, until_dt)
            if (cohort is None or int(row["market_id"]) in cohort)
            and (allowed_flat_names is None or str(row["trader_name"] or "") in allowed_flat_names)
        ]
        scoreable_rows: list[dict[str, Any]] = []
        for row in selected_decisions:
            market_id = int(row["market_id"])
            outcome = outcomes.get(market_id)
            forecast = _forecast(row)
            market_forecast = _clamp_probability(row["market_price"])
            if outcome is None or forecast is None or market_forecast is None:
                continue
            analysis_id = (
                str(row["analysis_batch_id"] or "").strip()
                if "analysis_batch_id" in row.keys()
                else ""
            )
            # Old databases had no batch identity. Treat each legacy row as a
            # unique batch rather than inventing false de-duplication.
            if not analysis_id:
                analysis_id = f"legacy-row:{int(row['id'])}"
            analysis_reference_price = (
                _clamp_probability(row["analysis_reference_price"])
                if "analysis_reference_price" in row.keys()
                else None
            )
            scoreable_rows.append(
                {
                    "id": int(row["id"]),
                    "persona": _persona_name(str(row["trader_name"])),
                    "trader_name": str(row["trader_name"]),
                    "market_id": market_id,
                    "market_name": str(row["market_name"] or ""),
                    "timestamp": str(row["timestamp"] or ""),
                    "analysis_batch_id": analysis_id,
                    "analysis_reference_price": analysis_reference_price,
                    "analysis_market_price": (
                        analysis_reference_price
                        if analysis_reference_price is not None
                        else market_forecast
                    ),
                    "forecast": forecast,
                    "raw_forecast": _raw_forecast(row),
                    "market_price": market_forecast,
                    "outcome": outcome,
                    "orders_count": _orders_count(row["orders"]),
                    "confidence": (
                        _clamp_probability(row["confidence"])
                        if "confidence" in row.keys()
                        else None
                    ),
                    "rejection_reason": (
                        str(row["rejection_reason"])
                        if "rejection_reason" in row.keys() and row["rejection_reason"]
                        else None
                    ),
                    "market_category": (
                        str(row["market_category"])
                        if "market_category" in row.keys() and row["market_category"]
                        else ""
                    ),
                    "market_tags": (
                        _json_string_list(row["market_tags"]) if "market_tags" in row.keys() else []
                    ),
                }
            )

        rows, duplicate_decision_rows_excluded = _deduplicate_analysis_batches(scoreable_rows)

        personas = []
        for persona in sorted({row["persona"] for row in rows}):
            subset = [row for row in rows if row["persona"] == persona]
            forecast_pairs = [(row["forecast"], row["outcome"]) for row in subset]
            raw_pairs = [
                (row["raw_forecast"], row["outcome"])
                for row in subset
                if row["raw_forecast"] is not None
            ]
            market_pairs = [(row["market_price"], row["outcome"]) for row in subset]
            personas.append(
                {
                    "persona": persona,
                    "n": len(subset),
                    "analysis_batch_count": len({row["analysis_batch_id"] for row in subset}),
                    "brier": _brier(forecast_pairs),
                    "raw_brier": _brier(raw_pairs),
                    "market_price_brier": _brier(market_pairs),
                    "mean_forecast": _mean([row["forecast"] for row in subset]),
                    "mean_outcome": _mean([row["outcome"] for row in subset]),
                    "mean_confidence": _mean(
                        [row["confidence"] for row in subset if row["confidence"] is not None]
                    ),
                    "reliability": _reliability_curve(forecast_pairs, bins),
                    "rejection_calibration": _rejection_calibration(subset),
                    "by_category_brier": _by_category_brier(subset),
                    "surprises": _surprises(subset, top_n),
                }
            )

        all_forecast_pairs = [(row["forecast"], row["outcome"]) for row in rows]
        all_market_pairs = [(row["market_price"], row["outcome"]) for row in rows]
        portfolio_pnl = {
            kind: _portfolio_pnl_summary(conn, kind, since_dt, until_dt, allowed_flat_names)
            for kind in ("flat", "kelly", "native_noise")
        }
        portfolio_pnl_by_trader = {
            kind: _portfolio_pnl_by_trader(conn, kind, since_dt, until_dt, allowed_flat_names)
            for kind in ("flat", "kelly", "native_noise")
        }
        result = {
            "db_path": str(Path(db_path)),
            "bins": bins,
            "resolved_threshold": resolved_threshold,
            "window": {
                "since": since_dt.isoformat() if since_dt is not None else None,
                "until": until_dt.isoformat() if until_dt is not None else None,
                "semantics": "since inclusive, until exclusive",
            },
            "cohort": {
                "requested_market_ids": sorted(cohort) if cohort is not None else None,
                "scored_market_ids": sorted({row["market_id"] for row in rows}),
            },
            "outcomes": {
                "source": outcome_source,
                "count": len(outcomes),
                "used_decision_rows": len(rows),
                "raw_scoreable_decision_rows": len(scoreable_rows),
                "duplicate_batch_decision_rows_excluded": duplicate_decision_rows_excluded,
            },
            "personas": personas,
            "baselines": {
                "market_price_as_forecast": {
                    "n": len(all_market_pairs),
                    "brier": _brier(all_market_pairs),
                },
                "native_noise_trader_pnl": portfolio_pnl["native_noise"],
            },
            "portfolio_pnl": portfolio_pnl,
            "portfolio_pnl_by_trader": portfolio_pnl_by_trader,
            "portfolio_pnl_scope": (
                "exact_experiment_flat_identities"
                if experiment is not None
                else "all_trader_positions"
            ),
            "analysis_batches": {
                "identity": "sha256(market_id + snapped reference price + sorted article URLs)",
                "scoring_semantics": (
                    "first forecast per durable trader_name + market_id + analysis_batch_id"
                ),
                "primary_experiment_comparison": "control_stage1_matching",
                "full_arm_metrics_semantics": (
                    "diagnostic only; unmatched analysis batches are included"
                ),
                "unique_scored_rows": len(rows),
                "duplicate_decision_rows_excluded": duplicate_decision_rows_excluded,
                "control_stage1_matching": _analysis_batch_matching(rows),
            },
            "overall": {
                "n": len(rows),
                "brier": _brier(all_forecast_pairs),
                "market_price_brier": _brier(all_market_pairs),
                "by_category_brier": _by_category_brier(rows),
            },
            "surprises": _surprises(rows, top_n),
        }
        if experiment is not None:
            result["experiment"] = {
                "experiment_id": experiment["experiment_id"],
                "mode": experiment["mode"],
                "configuration": experiment["configuration"],
                "window": {
                    "since": experiment["started_at_utc"].isoformat(),
                    "until": experiment["until_utc"].isoformat(),
                    "duration_seconds": experiment["duration_seconds"],
                    "minimum_strict_duration_seconds": MIN_EXPERIMENT_WINDOW_SECONDS,
                    "exploratory_short_window_override": experiment[
                        "exploratory_short_window_override"
                    ],
                },
                **_stage1_experiment_metrics(conn, experiment, selected_decisions),
            }
        return result
    finally:
        conn.close()


def _fmt(value: Any, precision: int = 4) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.{precision}f}"
    return str(value)


def format_report(result: dict[str, Any]) -> str:
    lines = []
    experiment = result.get("experiment")
    if experiment is not None:
        window = experiment["window"]
        window_label = (
            "EXPLORATORY SHORT-WINDOW OVERRIDE"
            if window["exploratory_short_window_override"]
            else "strict >=24h window"
        )
        lines.extend(
            [
                "Persisted Stage 1 experiment report",
                f"Experiment: {experiment['experiment_id']} ({experiment['mode']})",
                f"Window: [{window['since']}, {window['until']}) "
                f"duration={window['duration_seconds'] / 3600:.2f}h; {window_label}",
                "Identity scope: exact persisted experiment analyst/Flat identities only",
                "",
                "Stage 1 spend and PnL by exact arm",
            ]
        )
        for comparison in experiment["comparisons"]:
            lines.append(
                f"  {comparison['persona']}: matched-batches="
                f"{comparison['matched_analysis_batch_count']} "
                f"unmatched-control={comparison['unmatched_control_batch_count']} "
                f"unmatched-stage1={comparison['unmatched_stage1_batch_count']}"
            )
            for variant in ("control", "stage1"):
                arm = comparison[variant]
                lines.append(
                    f"    {variant:7s} calls={arm['calls']} usd={_fmt(arm['usd'], 5)} "
                    f"decisions={arm['decision_rows']} batches={arm['analysis_batch_count']} "
                    f"usd/decision={_fmt(arm['usd_per_decision'], 5)} "
                    f"usd/batch={_fmt(arm['usd_per_analysis_batch'], 5)} "
                    f"pnl={_fmt(arm['pnl'], 2)}"
                )
            delta = comparison["stage1_minus_control"]
            lines.append(
                "    Stage1-control delta: "
                f"calls={_fmt(delta['calls'])} usd={_fmt(delta['usd'], 5)} "
                f"usd/decision={_fmt(delta['usd_per_decision'], 5)} "
                f"usd/batch={_fmt(delta['usd_per_analysis_batch'], 5)} "
                f"pnl={_fmt(delta['pnl'], 2)}"
            )
        coverage = experiment["snapshot_coverage"]
        coverage_label = (
            "complete" if coverage["coverage_complete"] else "INCOMPLETE (exploratory report only)"
        )
        lines.extend(
            [
                "",
                "Portfolio snapshot window coverage: "
                f"{coverage_label}; expected cadence="
                f"{coverage['expected_cadence_seconds']}s, maximum gap/endpoint tolerance="
                f"{coverage['maximum_allowed_gap_seconds']}s",
            ]
        )
        for arm in coverage["arms"]:
            reasons = (
                "; ".join(arm["incomplete_reasons"]) if arm["incomplete_reasons"] else "complete"
            )
            lines.append(
                f"  {arm['persona']} {arm['variant']}: snapshots={arm['snapshot_count']} "
                f"first={arm['first_snapshot_utc'] or 'n/a'} "
                f"latest={arm['latest_snapshot_utc'] or 'n/a'} "
                f"max-gap={_fmt(arm['max_consecutive_gap_seconds'], 0)}s; {reasons}"
            )
        lines.append("")
        if result["outcomes"]["source"] == "explicit_unavailable":
            lines.extend(
                [
                    "No explicit outcomes: forecast metrics are unavailable; spend and PnL remain valid.",
                    "",
                ]
            )
    requested_market_ids = result.get("cohort", {}).get("requested_market_ids")
    if requested_market_ids is not None:
        lines.extend(
            [
                "Pinned forecast cohort: "
                + ",".join(str(market_id) for market_id in requested_market_ids),
                (
                    "Portfolio PnL scope: exact persisted experiment Flat identities"
                    if experiment is not None
                    else "Portfolio PnL scope: all trader positions; pin the same cohort in the runner"
                ),
                "",
            ]
        )
    matched_comparisons = result.get("analysis_batches", {}).get("control_stage1_matching", [])
    if matched_comparisons:
        lines.append("Stage 1 matched-batch experiment comparison (primary)")
        for comparison in matched_comparisons:
            label = f"{comparison['persona']} [SYB-114:{comparison['experiment_id']}]"
            if not comparison["comparable"]:
                lines.append(
                    f"  {label}: not comparable ({comparison['not_comparable_reason']}); "
                    f"unmatched-control={comparison['unmatched_control_count']} "
                    f"unmatched-stage1={comparison['unmatched_stage1_count']}"
                )
                continue
            control = comparison["control"]
            stage1 = comparison["stage1"]
            delta = comparison["stage1_minus_control"]
            lines.extend(
                [
                    f"  {label}: matched={comparison['matched_count']} "
                    f"unmatched-control={comparison['unmatched_control_count']} "
                    f"unmatched-stage1={comparison['unmatched_stage1_count']}",
                    f"    control n={control['n']} brier={_fmt(control['brier'])} "
                    f"market={_fmt(control['market_price_brier'])}",
                    f"    stage1  n={stage1['n']} brier={_fmt(stage1['brier'])} "
                    f"market={_fmt(stage1['market_price_brier'])}",
                    f"    Stage1-control delta: brier={_fmt(delta['brier'])} "
                    f"market={_fmt(delta['market_price_brier'])}",
                ]
            )
        lines.append("")
    persona_width = max([28, *(len(str(persona["persona"])) for persona in result["personas"])])
    lines.extend(
        [
            "Full-arm calibration by persona (diagnostic; unmatched batches included)",
            (
                f"{'persona':{persona_width}s}     n    brier  market   delta  "
                "reject  acted_b  reject_b  conf"
            ),
            "-" * (persona_width + 61),
        ]
    )
    for persona in result["personas"]:
        brier = persona["brier"]
        market = persona["market_price_brier"]
        delta = None if brier is None or market is None else brier - market
        rejection = persona["rejection_calibration"]
        lines.append(
            f"{persona['persona']:{persona_width}s} "
            f"{persona['n']:5d} "
            f"{_fmt(brier):>8s} "
            f"{_fmt(market):>7s} "
            f"{_fmt(delta):>7s} "
            f"{_fmt(rejection['rejection_rate'], 3):>7s} "
            f"{_fmt(rejection['acted_brier']):>8s} "
            f"{_fmt(rejection['rejected_brier']):>8s} "
            f"{_fmt(persona['mean_confidence'], 3):>6s}"
        )

    def pnl_line(label: str, summary: dict[str, Any]) -> str:
        return (
            f"{label}: n={summary.get('n', 0)} "
            f"mode={summary.get('mode', 'n/a')} "
            f"mean={_fmt(summary.get('mean_pnl'), 2)} "
            f"median={_fmt(summary.get('median_pnl'), 2)} "
            f"min={_fmt(summary.get('min_pnl'), 2)} "
            f"max={_fmt(summary.get('max_pnl'), 2)}"
        )

    portfolio_pnl = result["portfolio_pnl"]
    lines.extend(
        [
            "",
            (
                "Market-price baseline Brier: "
                f"{_fmt(result['baselines']['market_price_as_forecast']['brier'])} "
                f"(n={result['baselines']['market_price_as_forecast']['n']})"
            ),
            pnl_line("Flat-arm PnL", portfolio_pnl["flat"]),
            pnl_line("Kelly-arm PnL", portfolio_pnl["kelly"]),
            pnl_line("NativeNoiseTrader PnL baseline", portfolio_pnl["native_noise"]),
            (
                "Outcomes: "
                f"{result['outcomes']['count']} ({result['outcomes']['source']}), "
                f"decision rows used={result['outcomes']['used_decision_rows']}"
            ),
        ]
    )
    flat_by_trader = result.get("portfolio_pnl_by_trader", {}).get("flat", {})
    if flat_by_trader:
        lines.extend(
            ["", "Flat-arm PnL by durable trader identity"]
            + [f"  {name}: {_fmt(pnl, 2)}" for name, pnl in flat_by_trader.items()]
        )
    batch_summary = result.get("analysis_batches", {})
    lines.extend(
        [
            "",
            (
                "Analysis batches: unique scored rows="
                f"{batch_summary.get('unique_scored_rows', 0)}, duplicate decision rows excluded="
                f"{batch_summary.get('duplicate_decision_rows_excluded', 0)}"
            ),
        ]
    )
    for matching in batch_summary.get("control_stage1_matching", []):
        lines.append(
            f"  {matching['persona']} [SYB-114:{matching['experiment_id']}]: "
            f"matched={matching['matched_count']} "
            f"unmatched-control={matching['unmatched_control_count']} "
            f"unmatched-stage1={matching['unmatched_stage1_count']}"
        )
    reason_rows = []
    for persona in result["personas"]:
        for reason, stats in persona["rejection_calibration"]["by_reason"].items():
            reason_rows.append(
                f"  {persona['persona']} / {reason}: n={stats['n']} "
                f"would-have-profited={stats['would_have_profited_n']} "
                f"({_fmt(stats['would_have_profited_rate'], 3)})"
            )
    if reason_rows:
        lines.extend(["", "Rejection counterfactuals by reason", *reason_rows])

    categories = result["overall"]["by_category_brier"]
    if categories:
        lines.extend(["", "Brier by category"])
        lines.extend(
            f"  {item['category']}: n={item['n']} brier={_fmt(item['brier'])}"
            for item in categories
        )

    if result["surprises"]:
        lines.extend(["", "Largest submitted-order surprises"])
        lines.extend(
            f"  {item['trader_name']} market={item['market_id']} "
            f"forecast={_fmt(item['forecast'], 3)} outcome={_fmt(item['outcome'], 3)} "
            f"error={_fmt(item['absolute_error'], 3)}"
            for item in result["surprises"]
        )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Calibrate arena decision forecasts")
    parser.add_argument("--db", default="live/decisions.db", help="Path to decisions.db")
    parser.add_argument("--bins", type=int, default=10, help="Reliability curve bin count")
    parser.add_argument(
        "--resolved-threshold",
        type=float,
        default=0.95,
        help="Infer outcomes from final prices only outside this threshold",
    )
    parser.add_argument("--json-out", default="", help="Optional path to write JSON output")
    parser.add_argument("--since", default=None, help="Inclusive ISO timestamp window start")
    parser.add_argument("--until", default=None, help="Exclusive ISO timestamp window end")
    parser.add_argument(
        "--top-n", type=int, default=10, help="Number of submitted-order surprises to show"
    )
    parser.add_argument(
        "--market-ids",
        default=None,
        help="Comma-separated market IDs for an exact before/after cohort",
    )
    parser.add_argument(
        "--experiment-id",
        default=None,
        help=(
            "Strict report for one persisted concurrent Stage 1 experiment; derives the "
            "inclusive start and frozen market cohort and requires --until"
        ),
    )
    parser.add_argument(
        "--exploratory-short-window",
        action="store_true",
        help=(
            "Explicitly label and allow an experiment report shorter than 24 hours "
            "(recorded in text and JSON)"
        ),
    )
    args = parser.parse_args()

    result = analyze_decisions_db(
        args.db,
        bins=args.bins,
        resolved_threshold=args.resolved_threshold,
        since=args.since,
        until=args.until,
        top_n=args.top_n,
        market_ids=_parse_market_ids(args.market_ids),
        experiment_id=args.experiment_id,
        exploratory_short_window=args.exploratory_short_window,
    )
    print(format_report(result))
    json_text = json.dumps(result, indent=2, sort_keys=True)
    print("\nJSON:")
    print(json_text)
    if args.json_out:
        Path(args.json_out).write_text(json_text + "\n")


if __name__ == "__main__":
    main()
