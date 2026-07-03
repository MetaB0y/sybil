"""Shared data queries for dashboard and CLI status.

Single source of truth for all arena metrics. Returns DataFrames/dicts.
Rendering is the caller's job.
"""

import json
import os
import sqlite3

import pandas as pd

SYBIL_URL = os.environ.get("SYBIL_URL", "http://172.17.0.1:3000")


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


def get_latest_snapshots(conn: sqlite3.Connection) -> pd.DataFrame:
    """Latest portfolio snapshot per trader."""
    try:
        df = pd.read_sql_query(
            "SELECT trader_name, balance, portfolio_value, pnl, positions, total_fills, total_orders, timestamp "
            "FROM portfolio_snapshots WHERE id IN ("
            "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
            ") ORDER BY trader_name", conn
        )
    except Exception:
        df = pd.read_sql_query(
            "SELECT trader_name, balance, portfolio_value, pnl, positions, timestamp "
            "FROM portfolio_snapshots WHERE id IN ("
            "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
            ") ORDER BY trader_name", conn
        )
        df["total_fills"] = 0
        df["total_orders"] = 0
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

    agg = competing.groupby("strategy").agg(
        traders=("trader_name", "count"),
        total_pnl=("pnl", "sum"),
        avg_pnl=("pnl", "mean"),
    ).reset_index()

    # Count positions
    for idx, row in agg.iterrows():
        traders = competing[competing["strategy"] == row["strategy"]]
        n = 0
        for _, t in traders.iterrows():
            pos = json.loads(t["positions"]) if t["positions"] else {}
            n += sum(1 for mp in pos.values() for q in mp.values() if q != 0)
        agg.at[idx, "positions"] = int(n)

    # Average edge from decisions
    dec = pd.read_sql_query(
        "SELECT trader_name, AVG(ABS(fair_value - market_price)) as avg_edge "
        "FROM decisions GROUP BY trader_name", conn
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
    conn: sqlite3.Connection, traders: list[str] | None = None, cutoff: str | None = None,
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
        f"FROM decisions {clause} ORDER BY timestamp", conn
    )
    if df.empty:
        return pd.DataFrame()

    # Latest per (trader, market)
    latest = df.groupby(["trader_name", "market_name", "market_id"]).agg(
        current_fv=("fair_value", "last"),
        current_mkt=("market_price", "last"),
        n_decisions=("fair_value", "count"),
    ).reset_index()

    # Trend (last 5 FVs)
    def _trend(group):
        return " -> ".join(f"{v:.2f}" for v in group["fair_value"].tail(5))

    trends = df.groupby(["trader_name", "market_name"]).apply(
        _trend, include_groups=False
    ).reset_index(name="fv_trend")
    latest = latest.merge(trends, on=["trader_name", "market_name"], how="left")

    latest["edge"] = (latest["current_fv"] - latest["current_mkt"]).abs()
    latest["strategy"] = latest["trader_name"].apply(extract_strategy)
    latest["warning"] = latest.apply(
        lambda r: check_divergence(r["current_fv"], r["current_mkt"]), axis=1
    )
    return latest


def get_recent_decisions(
    conn: sqlite3.Connection, traders: list[str] | None = None,
    cutoff: str | None = None, limit: int = 50,
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
        f"FROM decisions {clause} ORDER BY id DESC LIMIT {limit}", conn
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
        f"FROM token_usage {clause} GROUP BY trader_name", conn
    )


def get_stats(conn: sqlite3.Connection) -> dict:
    return {
        "decisions": conn.execute("SELECT COUNT(*) FROM decisions").fetchone()[0],
        "articles": conn.execute("SELECT COUNT(*) FROM articles").fetchone()[0],
        "snapshots": conn.execute("SELECT COUNT(*) FROM portfolio_snapshots").fetchone()[0],
    }


def get_mm_mtm(sybil_url: str = SYBIL_URL, account_id: int = 0, initial_balance: float = 1_000_000.0) -> dict | None:
    """Fetch MM account from Sybil API and compute mark-to-market P&L.

    Returns dict with cash, position_value, total, pnl, return_pct, positions count,
    or None if the API is unreachable.
    """
    import urllib.request
    try:
        acct = json.loads(urllib.request.urlopen(f"{sybil_url}/v1/accounts/{account_id}", timeout=3).read())
        mkts = json.loads(urllib.request.urlopen(f"{sybil_url}/v1/markets?limit=2000", timeout=5).read())
    except Exception:
        return None

    ref_prices = {}
    for m in mkts:
        rp = m.get("reference_price_nanos")
        if rp and rp > 0:
            ref_prices[m["market_id"]] = rp / 1e9

    cash = acct["balance_nanos"] / 1e9
    position_value = 0.0
    n_positions = 0
    for p in acct.get("positions", []):
        mid = ref_prices.get(p["market_id"], 0.5)
        qty = p["quantity"]
        if p["outcome"] == "YES":
            position_value += qty * mid
        else:
            position_value += qty * (1.0 - mid)
        if qty != 0:
            n_positions += 1

    total = cash + position_value
    pnl = total - initial_balance
    return {
        "cash": cash,
        "position_value": position_value,
        "total": total,
        "pnl": pnl,
        "return_pct": pnl / initial_balance * 100 if initial_balance > 0 else 0,
        "positions": n_positions,
        "initial": initial_balance,
    }
