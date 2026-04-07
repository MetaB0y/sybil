"""Streamlit monitoring dashboard for live trading bots.

Usage:
    cd arena && uv run streamlit run live/dashboard.py
    # On server:
    streamlit run live/dashboard.py --server.port 8501 --server.address 0.0.0.0

Data queries are in live/queries.py (shared with live/status.py CLI).
"""

import json
import sqlite3
import os
from datetime import datetime, timedelta
from pathlib import Path

import altair as alt
import pandas as pd
import streamlit as st

from . import queries

DB_DEFAULT = os.environ.get(
    "ARENA_DB_PATH",
    "/data/decisions.db" if Path("/data").exists() else str(Path(__file__).parent / "decisions.db"),
)


def get_conn(db_path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(db_path, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


# --------------------------------------------------------------------------- #
# Page config
# --------------------------------------------------------------------------- #
st.set_page_config(page_title="Sybil Arena — Live", layout="wide")
st.title("Sybil Arena — Live Trading Dashboard")

# --------------------------------------------------------------------------- #
# Sidebar
# --------------------------------------------------------------------------- #
with st.sidebar:
    db_path = st.text_input("DB Path", value=DB_DEFAULT)
    auto_refresh = st.checkbox("Auto-refresh (30s)", value=True)
    time_range = st.selectbox("Time range", ["1h", "6h", "24h", "All"], index=2)
    strategy_filter = st.selectbox("Strategy", ["All", "Kelly", "Flat", "Legacy", "Noise"], index=0)

    if auto_refresh:
        st.markdown('<meta http-equiv="refresh" content="30">', unsafe_allow_html=True)

conn = get_conn(db_path)

# Time filter
time_map = {"1h": 1, "6h": 6, "24h": 24}
if time_range in time_map:
    cutoff = (datetime.utcnow() - timedelta(hours=time_map[time_range])).isoformat()
else:
    cutoff = None

# Trader filter
snaps_all = queries.get_latest_snapshots(conn)
all_traders = snaps_all["trader_name"].tolist() if not snaps_all.empty else []

if strategy_filter != "All":
    all_traders = [t for t in all_traders if queries.extract_strategy(t) == strategy_filter]

with st.sidebar:
    selected_traders = st.multiselect("Traders", all_traders, default=all_traders)

if not selected_traders:
    selected_traders = all_traders

# --------------------------------------------------------------------------- #
# 1. Strategy Comparison
# --------------------------------------------------------------------------- #
st.header("Strategy Comparison: Kelly vs Flat")

strat_df = queries.get_strategy_comparison(conn)
if strat_df is None:
    st.info("Waiting for portfolio snapshots...")
else:
    display = strat_df[["strategy", "traders", "total_pnl", "avg_pnl", "positions", "avg_edge"]].copy()
    display.columns = ["Strategy", "Traders", "Total PnL", "Avg PnL", "Positions", "Avg Edge"]
    display["Total PnL"] = display["Total PnL"].apply(lambda x: f"${x:+.2f}")
    display["Avg PnL"] = display["Avg PnL"].apply(lambda x: f"${x:+.2f}")
    display["Positions"] = display["Positions"].astype(int)
    display["Avg Edge"] = display["Avg Edge"].apply(lambda x: f"{x:.3f}")
    st.dataframe(display, use_container_width=True, hide_index=True)

    kelly_pnl = strat_df[strat_df["strategy"] == "Kelly"]["total_pnl"].sum()
    flat_pnl = strat_df[strat_df["strategy"] == "Flat"]["total_pnl"].sum()
    col1, col2, col3 = st.columns(3)
    col1.metric("Kelly Total PnL", f"${kelly_pnl:+.2f}")
    col2.metric("Flat Total PnL", f"${flat_pnl:+.2f}")
    leader = "Kelly" if kelly_pnl > flat_pnl else "Flat" if flat_pnl > kelly_pnl else "Tied"
    col3.metric("Leader", leader, delta=f"${abs(kelly_pnl - flat_pnl):.2f}")

# --------------------------------------------------------------------------- #
# 2. PnL Over Time
# --------------------------------------------------------------------------- #
st.header("PnL Over Time — Kelly vs Flat")

tlist = ",".join(f"'{t}'" for t in selected_traders)
pnl_query = (
    f"SELECT trader_name, timestamp, portfolio_value, pnl "
    f"FROM portfolio_snapshots WHERE trader_name IN ({tlist})"
    + (f" AND timestamp > '{cutoff}'" if cutoff else "")
    + " ORDER BY timestamp"
) if selected_traders else None

pnl_df = pd.read_sql_query(pnl_query, conn) if pnl_query else pd.DataFrame()

if pnl_df.empty:
    st.info("No PnL data yet.")
else:
    pnl_df["timestamp"] = pd.to_datetime(pnl_df["timestamp"])
    pnl_df["strategy"] = pnl_df["trader_name"].apply(queries.extract_strategy)

    strat_pnl = pnl_df.groupby(["timestamp", "strategy"])["pnl"].sum().reset_index()
    if not strat_pnl.empty:
        chart = (
            alt.Chart(strat_pnl)
            .mark_line(strokeWidth=3)
            .encode(
                x=alt.X("timestamp:T", title="Time"),
                y=alt.Y("pnl:Q", title="Total PnL ($)"),
                color=alt.Color("strategy:N", title="Strategy",
                                scale=alt.Scale(domain=["Kelly", "Flat", "Legacy", "Noise"],
                                                range=["#e45756", "#4c78a8", "#999", "#ccc"])),
                tooltip=["strategy", "timestamp:T", "pnl:Q"],
            )
            .properties(height=300)
            .interactive()
        )
        st.altair_chart(chart, use_container_width=True)

    with st.expander("Per-trader breakdown"):
        tchart = (
            alt.Chart(pnl_df)
            .mark_line(point=True)
            .encode(
                x=alt.X("timestamp:T", title="Time"),
                y=alt.Y("portfolio_value:Q", title="Portfolio Value ($)"),
                color=alt.Color("trader_name:N", title="Trader"),
                tooltip=["trader_name", "timestamp:T", "portfolio_value:Q", "pnl:Q"],
            )
            .properties(height=350)
            .interactive()
        )
        st.altair_chart(tchart, use_container_width=True)

# --------------------------------------------------------------------------- #
# 3. Fair Value Drift Monitor
# --------------------------------------------------------------------------- #
st.header("Fair Value Drift Monitor")

fv_df = queries.get_fv_drift(conn, traders=selected_traders, cutoff=cutoff)
if fv_df.empty:
    st.info("No decisions yet.")
else:
    warnings = fv_df[fv_df["warning"] != ""]
    if not warnings.empty:
        st.warning(f"{len(warnings)} divergent fair value(s) — possible conviction loop")

    display_fv = fv_df[["trader_name", "market_name", "current_fv", "current_mkt",
                         "edge", "fv_trend", "n_decisions", "warning"]].copy()
    display_fv.columns = ["Trader", "Market", "FV", "Mkt", "Edge", "Trend", "N", "Warning"]
    display_fv = display_fv.sort_values(["Warning", "Edge"], ascending=[True, False])
    display_fv["FV"] = display_fv["FV"].apply(lambda x: f"{x:.2f}")
    display_fv["Mkt"] = display_fv["Mkt"].apply(lambda x: f"{x:.2f}")
    display_fv["Edge"] = display_fv["Edge"].apply(lambda x: f"{x:.3f}")
    st.dataframe(display_fv, use_container_width=True, hide_index=True)

# --------------------------------------------------------------------------- #
# 4. Portfolio Summary
# --------------------------------------------------------------------------- #
st.header("Portfolio Summary")

if snaps_all.empty:
    st.info("No snapshots yet.")
else:
    filtered = snaps_all[snaps_all["trader_name"].isin(selected_traders)]
    if not filtered.empty:
        d = filtered[["trader_name", "strategy", "balance", "portfolio_value", "pnl", "timestamp"]].copy()
        d.columns = ["Trader", "Strategy", "Cash", "Portfolio Value", "PnL", "Last Updated"]
        d["Cash"] = d["Cash"].apply(lambda x: f"${x:.2f}")
        d["Portfolio Value"] = d["Portfolio Value"].apply(lambda x: f"${x:.2f}")
        d["PnL"] = d["PnL"].apply(lambda x: f"${x:+.2f}")
        st.dataframe(d, use_container_width=True, hide_index=True)

# --------------------------------------------------------------------------- #
# 5. Recent Decisions
# --------------------------------------------------------------------------- #
st.header("Recent Decisions")

dec_df = queries.get_recent_decisions(conn, traders=selected_traders, cutoff=cutoff, limit=50)
if dec_df.empty:
    st.info("No decisions yet.")
else:
    for _, row in dec_df.iterrows():
        orders = json.loads(row["orders"]) if row["orders"] else []
        edge = abs(row["fair_value"] - row["market_price"])
        orders_str = ", ".join(
            f"{o['side']} {o['qty']}@${o['price']:.2f}" for o in orders
        ) if orders else "HOLD"

        ts = row["timestamp"][:16] if row["timestamp"] else ""
        header = (f"**{row['trader_name']}** | {row['market_name'][:50]} | "
                  f"FV={row['fair_value']:.2f} vs Mkt={row['market_price']:.2f} (edge={edge:.2f}) | {orders_str}")

        with st.expander(f"{ts} — {header}"):
            st.markdown(f"**Analysis:** {row['analysis']}")
            st.markdown(f"**Motivation:** {row['motivation']}")
            st.markdown(f"**LLM latency:** {row['llm_duration_s']:.1f}s | **Balance:** ${row['balance']:.2f}")
            article_urls = json.loads(row["article_urls"]) if row.get("article_urls") else []
            if article_urls:
                st.markdown("**Sources:**")
                for art in article_urls:
                    st.markdown(f"- [{art['title'][:80]}]({art['url']}) ({art['source']})")

# --------------------------------------------------------------------------- #
# 6. News Feed
# --------------------------------------------------------------------------- #
st.header("News Feed")

art_clause = f"WHERE fetched_at > '{cutoff}'" if cutoff else ""
art_df = pd.read_sql_query(
    f"SELECT title, source, url, fetched_at, matched_market_ids FROM articles {art_clause} ORDER BY id DESC LIMIT 30",
    conn,
)
if art_df.empty:
    st.info("No articles yet.")
else:
    for _, row in art_df.iterrows():
        mids = json.loads(row["matched_market_ids"]) if row["matched_market_ids"] else []
        ts = row["fetched_at"][:16] if row["fetched_at"] else ""
        title = row["title"] or "(no title)"
        url = row["url"] or ""
        if url:
            st.markdown(f"- **{ts}** [{row['source']}] [{title}]({url}) -> {len(mids)} market(s)")
        else:
            st.markdown(f"- **{ts}** [{row['source']}] {title} -> {len(mids)} market(s)")

# --------------------------------------------------------------------------- #
# 7. LLM Cost
# --------------------------------------------------------------------------- #
st.header("LLM Cost Tracker")

cost_df = queries.get_llm_cost(conn, cutoff=cutoff)
if cost_df is None:
    st.info("Token usage tracking not available.")
elif cost_df.empty:
    st.info("No token usage yet.")
else:
    cost_df["est_cost"] = (cost_df["prompt_tokens"] + cost_df["completion_tokens"]) * 0.70 / 1_000_000
    d = cost_df.rename(columns={
        "trader_name": "Trader", "calls": "Calls",
        "prompt_tokens": "Prompt", "completion_tokens": "Completion",
        "avg_latency_s": "Avg Latency (s)", "est_cost": "Est. Cost ($)",
    })
    d["Est. Cost ($)"] = d["Est. Cost ($)"].apply(lambda x: f"${x:.4f}")
    d["Avg Latency (s)"] = d["Avg Latency (s)"].apply(lambda x: f"{x:.1f}")
    st.dataframe(d, use_container_width=True, hide_index=True)

    total = cost_df["prompt_tokens"].sum() + cost_df["completion_tokens"].sum()
    st.metric("Total Estimated Cost", f"${total * 0.70 / 1_000_000:.4f}")

# --------------------------------------------------------------------------- #
# 8. Stats
# --------------------------------------------------------------------------- #
st.header("Stats")
stats = queries.get_stats(conn)
col1, col2, col3 = st.columns(3)
col1.metric("Decisions", stats["decisions"])
col2.metric("Articles", stats["articles"])
col3.metric("Snapshots", stats["snapshots"])

conn.close()
