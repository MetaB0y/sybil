"""Shared data queries for dashboard and CLI status.

Single source of truth for all arena metrics. Returns DataFrames/dicts.
Rendering is the caller's job.
"""

import json
import os
import sqlite3
from hashlib import sha256

import pandas as pd

try:
    from .personas import PERSONAS
except ImportError:
    from personas import PERSONAS  # type: ignore[no-redef]

SYBIL_URL = os.environ.get("SYBIL_URL", "http://172.17.0.1:3000")
RUNTIME_HEARTBEAT_MAX_AGE = "-15 minutes"


def get_active_scored_runtime(
    conn: sqlite3.Connection,
) -> tuple[str, list[str]] | None:
    """Return the live scored cohort start and names, or no live cohort.

    Historical snapshots remain durable diagnostics. Competition aggregates
    must be explicitly scoped to a recently heartbeating runtime so old bot
    identities and synthetic load do not silently change the reported total.
    """
    required = {"arena_runs", "arena_run_participants"}
    tables = {
        str(row[0])
        for row in conn.execute("SELECT name FROM sqlite_master WHERE type = 'table'").fetchall()
    }
    if not required.issubset(tables):
        return None

    run = conn.execute(
        "SELECT run_id, started_at_utc FROM arena_runs "
        "WHERE stopped_at_utc IS NULL "
        "  AND julianday(heartbeat_at_utc) >= julianday('now', ?) "
        "ORDER BY started_at_utc DESC LIMIT 1",
        (RUNTIME_HEARTBEAT_MAX_AGE,),
    ).fetchone()
    if run is None:
        return None

    names = [
        str(row[0])
        for row in conn.execute(
            "SELECT trader_name FROM arena_run_participants "
            "WHERE run_id = ? AND scored = 1 ORDER BY trader_name",
            (run[0],),
        ).fetchall()
    ]
    return str(run[1]), names


def extract_strategy(name: str) -> str:
    if "(Kelly)" in name:
        return "Kelly"
    elif "(Flat)" in name:
        return "Flat"
    elif name.startswith("Noise"):
        return "Noise"
    return "Legacy"


def check_divergence(fv: float, mkt: float) -> str:
    """Flag FVs that are extreme AND disagree with market."""
    fv_extreme = fv > 0.85 or fv < 0.15
    mkt_agrees = (mkt > 0.80 and fv > 0.85) or (mkt < 0.20 and fv < 0.15)
    return "DIVERGENT" if fv_extreme and not mkt_agrees else ""


def get_latest_snapshots(conn: sqlite3.Connection, *, scored_only: bool = True) -> pd.DataFrame:
    """Latest portfolio snapshot per trader, scoped to the live score cohort."""
    try:
        df = pd.read_sql_query(
            "SELECT trader_name, balance, portfolio_value, pnl, positions, total_fills, total_orders, timestamp "
            "FROM portfolio_snapshots WHERE id IN ("
            "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
            ") ORDER BY trader_name",
            conn,
        )
    except Exception:
        df = pd.read_sql_query(
            "SELECT trader_name, balance, portfolio_value, pnl, positions, timestamp "
            "FROM portfolio_snapshots WHERE id IN ("
            "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
            ") ORDER BY trader_name",
            conn,
        )
        df["total_fills"] = 0
        df["total_orders"] = 0
    if scored_only:
        runtime = get_active_scored_runtime(conn)
        if runtime is None:
            return df.iloc[0:0].copy()
        _started_at, names = runtime
        df = df[df["trader_name"].isin(names)].copy()
    if not df.empty:
        df["strategy"] = df["trader_name"].apply(extract_strategy)
    return df


def get_strategy_comparison(conn: sqlite3.Connection) -> pd.DataFrame | None:
    """Aggregate Kelly vs Flat stats. Returns None if no data."""
    snaps = get_latest_snapshots(conn)
    if snaps.empty:
        return None

    competing = snaps[snaps["strategy"].isin(["Kelly", "Flat"])]
    if competing.empty:
        return None

    agg = (
        competing.groupby("strategy")
        .agg(
            traders=("trader_name", "count"),
            total_pnl=("pnl", "sum"),
            avg_pnl=("pnl", "mean"),
        )
        .reset_index()
    )

    # Count positions
    for idx, row in agg.iterrows():
        traders = competing[competing["strategy"] == row["strategy"]]
        n = 0
        for _, t in traders.iterrows():
            pos = json.loads(t["positions"]) if t["positions"] else {}
            n += sum(1 for mp in pos.values() for q in mp.values() if q != 0)
        agg.at[idx, "positions"] = int(n)

    # Average edge from decisions
    runtime = get_active_scored_runtime(conn)
    started_at = runtime[0] if runtime is not None else ""
    dec = pd.read_sql_query(
        "SELECT trader_name, AVG(ABS(fair_value - market_price)) as avg_edge "
        "FROM decisions WHERE timestamp >= ? GROUP BY trader_name",
        conn,
        params=(started_at,),
    )
    if not dec.empty:
        dec["strategy"] = dec["trader_name"].apply(extract_strategy)
        dec = dec[dec["strategy"].isin(["Kelly", "Flat"])]
        edge = dec.groupby("strategy")["avg_edge"].mean().reset_index()
        agg = agg.merge(edge, on="strategy", how="left")
    else:
        agg["avg_edge"] = 0.0

    return agg


def get_fv_drift(
    conn: sqlite3.Connection,
    traders: list[str] | None = None,
    cutoff: str | None = None,
) -> pd.DataFrame:
    """Fair value drift per (trader, market) with divergence warnings."""
    where = []
    if traders:
        tlist = ",".join(f"'{t}'" for t in traders)
        where.append(f"trader_name IN ({tlist})")
    if cutoff:
        where.append(f"timestamp > '{cutoff}'")
    clause = "WHERE " + " AND ".join(where) if where else ""

    df = pd.read_sql_query(
        f"SELECT trader_name, market_name, market_id, fair_value, market_price, timestamp "
        f"FROM decisions {clause} ORDER BY timestamp",
        conn,
    )
    if df.empty:
        return pd.DataFrame()

    # Latest per (trader, market)
    latest = (
        df.groupby(["trader_name", "market_name", "market_id"])
        .agg(
            current_fv=("fair_value", "last"),
            current_mkt=("market_price", "last"),
            n_decisions=("fair_value", "count"),
        )
        .reset_index()
    )

    # Trend (last 5 FVs)
    def _trend(group):
        return " -> ".join(f"{v:.2f}" for v in group["fair_value"].tail(5))

    trends = (
        df.groupby(["trader_name", "market_name"])
        .apply(_trend, include_groups=False)
        .reset_index(name="fv_trend")
    )
    latest = latest.merge(trends, on=["trader_name", "market_name"], how="left")

    latest["edge"] = (latest["current_fv"] - latest["current_mkt"]).abs()
    latest["strategy"] = latest["trader_name"].apply(extract_strategy)
    latest["warning"] = latest.apply(
        lambda r: check_divergence(r["current_fv"], r["current_mkt"]), axis=1
    )
    return latest


def get_recent_decisions(
    conn: sqlite3.Connection,
    traders: list[str] | None = None,
    cutoff: str | None = None,
    limit: int = 50,
) -> pd.DataFrame:
    where = []
    if traders:
        tlist = ",".join(f"'{t}'" for t in traders)
        where.append(f"trader_name IN ({tlist})")
    if cutoff:
        where.append(f"timestamp > '{cutoff}'")
    clause = "WHERE " + " AND ".join(where) if where else ""

    return pd.read_sql_query(
        f"SELECT trader_name, market_name, fair_value, market_price, orders, "
        f"       motivation, analysis, llm_duration_s, timestamp, balance, article_urls "
        f"FROM decisions {clause} ORDER BY id DESC LIMIT {limit}",
        conn,
    )


def get_llm_cost(conn: sqlite3.Connection, cutoff: str | None = None) -> pd.DataFrame | None:
    has = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='token_usage'"
    ).fetchone()
    if not has:
        return None

    clause = f"WHERE timestamp > '{cutoff}'" if cutoff else ""
    return pd.read_sql_query(
        f"SELECT trader_name, COUNT(*) as calls, "
        f"  SUM(prompt_tokens) as prompt_tokens, "
        f"  SUM(completion_tokens) as completion_tokens, "
        f"  AVG(duration_s) as avg_latency_s "
        f"FROM token_usage {clause} GROUP BY trader_name",
        conn,
    )


def get_stats(conn: sqlite3.Connection) -> dict:
    return {
        "decisions": conn.execute("SELECT COUNT(*) FROM decisions").fetchone()[0],
        "articles": conn.execute("SELECT COUNT(*) FROM articles").fetchone()[0],
        "snapshots": conn.execute("SELECT COUNT(*) FROM portfolio_snapshots").fetchone()[0],
    }


def get_live_experiment_status(conn: sqlite3.Connection) -> list[dict]:
    """Return persisted experiment metadata plus per-arm durable activity.

    Older decision databases legitimately have no ``live_experiments`` table,
    so status remains backward-compatible and simply reports no experiments.
    Only exact durable Stage 1 Flat trader names count toward arm activity;
    ordinary Kelly/Flat and synthetic accounts cannot inflate readiness.
    """
    has_experiments = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='live_experiments'"
    ).fetchone()
    if not has_experiments:
        return []

    experiments = conn.execute(
        "SELECT experiment_id, mode, started_at_utc, configuration_json "
        "FROM live_experiments ORDER BY started_at_utc DESC"
    ).fetchall()
    if not experiments:
        return []

    activity: dict[str, dict[str, dict]] = {}

    def collect(table: str, count_key: str) -> None:
        has_table = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
            (table,),
        ).fetchone()
        if not has_table:
            return
        rows = conn.execute(
            f"SELECT trader_name, COUNT(*) AS row_count, "  # noqa: S608 -- fixed table names
            f"MIN(timestamp) AS first_at, MAX(timestamp) AS last_at FROM {table} "
            "GROUP BY trader_name"
        ).fetchall()
        for row in rows:
            trader_name = str(row[0])
            record = activity.setdefault(trader_name, {}).setdefault(
                count_key,
                {
                    "count": 0,
                    "first_at": None,
                    "last_at": None,
                },
            )
            record["count"] += int(row[1])
            if row[2] is not None:
                record["first_at"] = min(filter(None, (record["first_at"], str(row[2]))))
            if row[3] is not None:
                record["last_at"] = max(filter(None, (record["last_at"], str(row[3]))))

    collect("decisions", "decision_count")
    collect("portfolio_snapshots", "snapshot_count")

    result = []
    for row in experiments:
        experiment_id = str(row[0])
        configuration = json.loads(str(row[3]))
        persona_keys = configuration.get("personas")
        display_hashes = configuration.get("persona_display_name_sha256")
        identity_error = None
        expected_names: dict[str, set[str]] = {"control": set(), "stage1": set()}
        if (
            not isinstance(persona_keys, list)
            or not persona_keys
            or not all(isinstance(persona_key, str) for persona_key in persona_keys)
            or len(set(persona_keys)) != len(persona_keys)
            or not isinstance(display_hashes, dict)
        ):
            identity_error = "experiment lacks immutable persona identity metadata"
        else:
            for persona_key in persona_keys:
                persona = PERSONAS.get(persona_key) if isinstance(persona_key, str) else None
                if persona is None:
                    identity_error = f"unknown persisted persona identity: {persona_key!r}"
                    break
                display_name = str(persona["name"])
                fingerprint = sha256(display_name.encode("utf-8")).hexdigest()
                if display_hashes.get(persona_key) != fingerprint:
                    identity_error = f"display-name fingerprint drift: {persona_key!r}"
                    break
                for variant in ("control", "stage1"):
                    expected_names[variant].add(
                        f"{display_name} [SYB-114:{experiment_id}:{variant}] (Flat)"
                    )
        expected_traders = 0 if identity_error else len(persona_keys)
        arms = {}
        for variant in ("control", "stage1"):
            expected = expected_names[variant] if identity_error is None else set()
            decision_names = {
                name for name in expected if activity.get(name, {}).get("decision_count")
            }
            snapshot_names = {
                name for name in expected if activity.get(name, {}).get("snapshot_count")
            }

            def summarize(kind: str, names: set[str]) -> tuple[int, str | None, str | None]:
                records = [activity[name][kind] for name in names]
                return (
                    sum(record["count"] for record in records),
                    min(
                        (record["first_at"] for record in records if record["first_at"]),
                        default=None,
                    ),
                    max(
                        (record["last_at"] for record in records if record["last_at"]),
                        default=None,
                    ),
                )

            decision_count, first_decision, last_decision = summarize(
                "decision_count", decision_names
            )
            snapshot_count, first_snapshot, last_snapshot = summarize(
                "snapshot_count", snapshot_names
            )
            arms[variant] = {
                "decision_count": decision_count,
                "decision_traders": len(decision_names),
                "first_decision_at": first_decision,
                "last_decision_at": last_decision,
                "snapshot_count": snapshot_count,
                "snapshot_traders": len(snapshot_names),
                "first_snapshot_at": first_snapshot,
                "last_snapshot_at": last_snapshot,
                "ready": (
                    bool(expected) and decision_names == expected and snapshot_names == expected
                ),
            }
        result.append(
            {
                "experiment_id": experiment_id,
                "mode": str(row[1]),
                "started_at_utc": str(row[2]),
                "configuration": configuration,
                "expected_traders_per_arm": expected_traders,
                "identity_error": identity_error,
                "arms": arms,
            }
        )
    return result


def get_mm_mtm(sybil_url: str = SYBIL_URL, account_id: int = 1) -> dict | None:
    """Fetch the canonical Sybil-marked MM portfolio.

    Returns dict with cash, position_value, total, pnl, return_pct, positions count,
    or None if the API is unreachable.
    """
    import urllib.request

    token = os.environ.get("SYBIL_SERVICE_TOKEN", "")

    def private_request(path: str):
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        request = urllib.request.Request(f"{sybil_url}{path}", headers=headers)
        return json.loads(urllib.request.urlopen(request, timeout=5).read())

    try:
        portfolio = private_request(f"/v1/accounts/{account_id}/portfolio")
    except Exception:
        return None

    cash = portfolio["balance_nanos"] / 1e9
    position_value = portfolio["total_position_value_nanos"] / 1e9
    total = portfolio["portfolio_value_nanos"] / 1e9
    pnl = portfolio["pnl_nanos"] / 1e9
    deposited = portfolio["total_deposited_nanos"] / 1e9
    n_positions = sum(1 for position in portfolio.get("positions", []) if position["quantity"] != 0)
    return {
        "cash": cash,
        "position_value": position_value,
        "total": total,
        "pnl": pnl,
        "return_pct": pnl / deposited * 100 if deposited > 0 else 0,
        "positions": n_positions,
        "initial": deposited,
    }
