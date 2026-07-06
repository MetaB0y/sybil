"""Offline calibration report for live arena decisions.

Usage:
    cd arena
    uv run python -m scripts.calibration --db live/decisions.db
    uv run python -m scripts.calibration --db live/decisions.db --json-out calibration.json
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sqlite3
import statistics
from pathlib import Path
from typing import Any


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
        "SELECT market_id, market_price FROM decisions "
        "WHERE market_price IS NOT NULL ORDER BY id"
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


def _select_decisions(conn: sqlite3.Connection) -> list[sqlite3.Row]:
    cols = _columns(conn, "decisions")
    if not cols:
        return []
    optional = [
        "raw_fair_value",
        "effective_fair_value",
        "fair_value_age_s",
        "confidence",
        "countercase",
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
    return list(conn.execute(f"SELECT {', '.join(selected)} FROM decisions ORDER BY id"))


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
        curve.append({
            "bin": idx,
            "low": idx / bins,
            "high": (idx + 1) / bins,
            "n": len(bucket),
            "mean_forecast": _mean(forecasts),
            "empirical_yes_rate": _mean(outcomes),
            "brier": _brier(bucket),
        })
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
    }


def _native_noise_pnl_baseline(conn: sqlite3.Connection) -> dict[str, Any]:
    if not _has_table(conn, "portfolio_snapshots"):
        return {"n": 0}
    rows = conn.execute(
        "SELECT trader_name, pnl FROM portfolio_snapshots WHERE id IN ("
        "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
        ")"
    )
    pnls = [
        float(row["pnl"])
        for row in rows
        if row["pnl"] is not None
        and (
            str(row["trader_name"]).startswith("Noise")
            or "NativeNoiseTrader" in str(row["trader_name"])
        )
    ]
    if not pnls:
        return {"n": 0}
    return {
        "n": len(pnls),
        "mean_pnl": statistics.fmean(pnls),
        "median_pnl": statistics.median(pnls),
        "min_pnl": min(pnls),
        "max_pnl": max(pnls),
    }


def analyze_decisions_db(
    db_path: str,
    bins: int = 10,
    resolved_threshold: float = 0.95,
) -> dict[str, Any]:
    if bins <= 0:
        raise ValueError("bins must be positive")
    if not 0.5 < resolved_threshold <= 1.0:
        raise ValueError("resolved_threshold must be in (0.5, 1.0]")

    conn = _connect(db_path)
    try:
        outcomes, outcome_source = load_outcomes(conn, resolved_threshold)
        rows: list[dict[str, Any]] = []
        for row in _select_decisions(conn):
            outcome = outcomes.get(int(row["market_id"]))
            forecast = _forecast(row)
            market_forecast = _clamp_probability(row["market_price"])
            if outcome is None or forecast is None or market_forecast is None:
                continue
            rows.append({
                "persona": _persona_name(str(row["trader_name"])),
                "trader_name": str(row["trader_name"]),
                "market_id": int(row["market_id"]),
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
            })

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
            personas.append({
                "persona": persona,
                "n": len(subset),
                "brier": _brier(forecast_pairs),
                "raw_brier": _brier(raw_pairs),
                "market_price_brier": _brier(market_pairs),
                "mean_forecast": _mean([row["forecast"] for row in subset]),
                "mean_outcome": _mean([row["outcome"] for row in subset]),
                "mean_confidence": _mean([
                    row["confidence"]
                    for row in subset
                    if row["confidence"] is not None
                ]),
                "reliability": _reliability_curve(forecast_pairs, bins),
                "rejection_calibration": _rejection_calibration(subset),
            })

        all_forecast_pairs = [(row["forecast"], row["outcome"]) for row in rows]
        all_market_pairs = [(row["market_price"], row["outcome"]) for row in rows]
        return {
            "db_path": str(Path(db_path)),
            "bins": bins,
            "resolved_threshold": resolved_threshold,
            "outcomes": {
                "source": outcome_source,
                "count": len(outcomes),
                "used_decision_rows": len(rows),
            },
            "personas": personas,
            "baselines": {
                "market_price_as_forecast": {
                    "n": len(all_market_pairs),
                    "brier": _brier(all_market_pairs),
                },
                "native_noise_trader_pnl": _native_noise_pnl_baseline(conn),
            },
            "overall": {
                "n": len(rows),
                "brier": _brier(all_forecast_pairs),
                "market_price_brier": _brier(all_market_pairs),
            },
        }
    finally:
        conn.close()


def _fmt(value: Any, precision: int = 4) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.{precision}f}"
    return str(value)


def format_report(result: dict[str, Any]) -> str:
    lines = [
        "Calibration by persona",
        (
            "persona                         n    brier  market   delta  "
            "reject  acted_b  reject_b  conf"
        ),
        "-" * 89,
    ]
    for persona in result["personas"]:
        brier = persona["brier"]
        market = persona["market_price_brier"]
        delta = None if brier is None or market is None else brier - market
        rejection = persona["rejection_calibration"]
        lines.append(
            f"{persona['persona'][:28]:28s} "
            f"{persona['n']:5d} "
            f"{_fmt(brier):>8s} "
            f"{_fmt(market):>7s} "
            f"{_fmt(delta):>7s} "
            f"{_fmt(rejection['rejection_rate'], 3):>7s} "
            f"{_fmt(rejection['acted_brier']):>8s} "
            f"{_fmt(rejection['rejected_brier']):>8s} "
            f"{_fmt(persona['mean_confidence'], 3):>6s}"
        )

    baseline = result["baselines"]["native_noise_trader_pnl"]
    lines.extend([
        "",
        (
            "Market-price baseline Brier: "
            f"{_fmt(result['baselines']['market_price_as_forecast']['brier'])} "
            f"(n={result['baselines']['market_price_as_forecast']['n']})"
        ),
        (
            "NativeNoiseTrader PnL baseline: "
            f"n={baseline.get('n', 0)} "
            f"mean={_fmt(baseline.get('mean_pnl'), 2)} "
            f"median={_fmt(baseline.get('median_pnl'), 2)} "
            f"min={_fmt(baseline.get('min_pnl'), 2)} "
            f"max={_fmt(baseline.get('max_pnl'), 2)}"
        ),
        (
            "Outcomes: "
            f"{result['outcomes']['count']} ({result['outcomes']['source']}), "
            f"decision rows used={result['outcomes']['used_decision_rows']}"
        ),
    ])
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
    args = parser.parse_args()

    result = analyze_decisions_db(args.db, bins=args.bins, resolved_threshold=args.resolved_threshold)
    print(format_report(result))
    json_text = json.dumps(result, indent=2, sort_keys=True)
    print("\nJSON:")
    print(json_text)
    if args.json_out:
        Path(args.json_out).write_text(json_text + "\n")


if __name__ == "__main__":
    main()
