"""Streamlit monitoring dashboard for live trading bots.

Usage:
    cd arena && uv run streamlit run live/dashboard.py
    # On server:
    streamlit run live/dashboard.py --server.port 8501 --server.address 0.0.0.0
"""

import json
import sqlite3
from datetime import datetime, timedelta
from pathlib import Path

import altair as alt
import pandas as pd
import streamlit as st

import os

# In Docker, the DB is on the shared volume at /data/decisions.db
# Locally, it's in the live/ directory
DB_DEFAULT = os.environ.get(
    "ARENA_DB_PATH",
    "/data/decisions.db" if Path("/data").exists() else str(Path(__file__).parent / "decisions.db"),
)


def get_conn(db_path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(db_path, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def extract_strategy(trader_name: str) -> str:
    """Extract strategy label from trader name like 'News Trader (Kelly)'."""
    if "(Kelly)" in trader_name:
        return "Kelly"
    elif "(Flat)" in trader_name:
        return "Flat"
    return "Unknown"


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
    strategy_filter = st.selectbox("Strategy", ["All", "Kelly", "Flat"], index=0)

    if auto_refresh:
        st.markdown(
            '<meta http-equiv="refresh" content="30">',
            unsafe_allow_html=True,
        )

conn = get_conn(db_path)

# Compute time filter
time_filters = {
    "1h": timedelta(hours=1),
    "6h": timedelta(hours=6),
    "24h": timedelta(hours=24),
}
if time_range in time_filters:
    cutoff = (datetime.utcnow() - time_filters[time_range]).isoformat()
    time_clause = f"WHERE timestamp > '{cutoff}'"
    time_clause_snap = f"WHERE timestamp > '{cutoff}'"
else:
    cutoff = None
    time_clause = ""
    time_clause_snap = ""

# --------------------------------------------------------------------------- #
# Trader filter
# --------------------------------------------------------------------------- #
traders_df = pd.read_sql_query(
    "SELECT DISTINCT trader_name FROM portfolio_snapshots "
    "UNION SELECT DISTINCT trader_name FROM decisions ORDER BY 1", conn
)
all_traders = traders_df["trader_name"].tolist() if not traders_df.empty else []

# Apply strategy filter
if strategy_filter != "All":
    all_traders = [t for t in all_traders if f"({strategy_filter})" in t]

with st.sidebar:
    selected_traders = st.multiselect("Traders", all_traders, default=all_traders)

if not selected_traders:
    selected_traders = all_traders
trader_filter = ",".join(f"'{t}'" for t in selected_traders)

# --------------------------------------------------------------------------- #
# 1. Strategy Comparison (headline section)
# --------------------------------------------------------------------------- #
st.header("Strategy Comparison: Kelly vs Flat")

snap_all = pd.read_sql_query(
    "SELECT trader_name, balance, portfolio_value, pnl, positions, timestamp "
    "FROM portfolio_snapshots WHERE id IN ("
    "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
    ") ORDER BY trader_name", conn
)

if snap_all.empty:
    st.info("Waiting for portfolio snapshots to compare strategies...")
else:
    snap_all["strategy"] = snap_all["trader_name"].apply(extract_strategy)

    # Aggregate by strategy
    strat_agg = snap_all.groupby("strategy").agg(
        traders=("trader_name", "count"),
        total_pnl=("pnl", "sum"),
        avg_pnl=("pnl", "mean"),
        total_value=("portfolio_value", "sum"),
        avg_cash=("balance", "mean"),
    ).reset_index()

    # Count positions per strategy
    for idx, row in strat_agg.iterrows():
        strategy_traders = snap_all[snap_all["strategy"] == row["strategy"]]
        total_positions = 0
        for _, t in strategy_traders.iterrows():
            positions = json.loads(t["positions"]) if t["positions"] else {}
            total_positions += sum(
                1 for mid_pos in positions.values()
                for qty in mid_pos.values() if qty != 0
            )
        strat_agg.at[idx, "positions"] = total_positions

    # Get average edge from decisions
    dec_edge = pd.read_sql_query(
        "SELECT trader_name, AVG(ABS(fair_value - market_price)) as avg_edge "
        "FROM decisions GROUP BY trader_name", conn
    )
    if not dec_edge.empty:
        dec_edge["strategy"] = dec_edge["trader_name"].apply(extract_strategy)
        edge_by_strat = dec_edge.groupby("strategy")["avg_edge"].mean().reset_index()
        strat_agg = strat_agg.merge(edge_by_strat, on="strategy", how="left")
    else:
        strat_agg["avg_edge"] = 0.0

    # Display
    display_strat = strat_agg[["strategy", "traders", "total_pnl", "avg_pnl", "positions", "avg_edge"]].copy()
    display_strat.columns = ["Strategy", "Traders", "Total PnL", "Avg PnL", "Positions", "Avg Edge"]
    display_strat["Total PnL"] = display_strat["Total PnL"].apply(lambda x: f"${x:+.2f}")
    display_strat["Avg PnL"] = display_strat["Avg PnL"].apply(lambda x: f"${x:+.2f}")
    display_strat["Positions"] = display_strat["Positions"].astype(int)
    display_strat["Avg Edge"] = display_strat["Avg Edge"].apply(lambda x: f"{x:.3f}")
    st.dataframe(display_strat, use_container_width=True, hide_index=True)

    # Quick metric cards
    kelly_pnl = snap_all[snap_all["strategy"] == "Kelly"]["pnl"].sum()
    flat_pnl = snap_all[snap_all["strategy"] == "Flat"]["pnl"].sum()
    col1, col2, col3 = st.columns(3)
    col1.metric("Kelly Total PnL", f"${kelly_pnl:+.2f}")
    col2.metric("Flat Total PnL", f"${flat_pnl:+.2f}")
    leader = "Kelly" if kelly_pnl > flat_pnl else "Flat" if flat_pnl > kelly_pnl else "Tied"
    col3.metric("Leader", leader, delta=f"${abs(kelly_pnl - flat_pnl):.2f}")

# --------------------------------------------------------------------------- #
# 2. PnL Over Time (strategy overlay)
# --------------------------------------------------------------------------- #
st.header("PnL Over Time — Kelly vs Flat")

pnl_query = (
    f"SELECT trader_name, timestamp, portfolio_value, pnl "
    f"FROM portfolio_snapshots "
    f"WHERE trader_name IN ({trader_filter})"
    + (f" AND timestamp > '{cutoff}'" if cutoff else "")
    + " ORDER BY timestamp"
)
pnl_df = pd.read_sql_query(pnl_query, conn)

if pnl_df.empty:
    st.info("No data for PnL chart yet.")
else:
    pnl_df["timestamp"] = pd.to_datetime(pnl_df["timestamp"])
    pnl_df["strategy"] = pnl_df["trader_name"].apply(extract_strategy)

    # Strategy aggregate PnL over time
    strat_pnl = pnl_df.groupby(["timestamp", "strategy"])["pnl"].sum().reset_index()

    if not strat_pnl.empty:
        strat_chart = (
            alt.Chart(strat_pnl)
            .mark_line(strokeWidth=3)
            .encode(
                x=alt.X("timestamp:T", title="Time"),
                y=alt.Y("pnl:Q", title="Total PnL ($)"),
                color=alt.Color("strategy:N", title="Strategy",
                                scale=alt.Scale(domain=["Kelly", "Flat"],
                                                range=["#e45756", "#4c78a8"])),
                tooltip=["strategy", "timestamp:T", "pnl:Q"],
            )
            .properties(height=300)
            .interactive()
        )
        st.altair_chart(strat_chart, use_container_width=True)

    # Per-trader detail (collapsible)
    with st.expander("Per-trader breakdown"):
        trader_chart = (
            alt.Chart(pnl_df)
            .mark_line(point=True)
            .encode(
                x=alt.X("timestamp:T", title="Time"),
                y=alt.Y("portfolio_value:Q", title="Portfolio Value ($)"),
                color=alt.Color("trader_name:N", title="Trader"),
                strokeDash=alt.StrokeDash(
                    "strategy:N",
                    scale=alt.Scale(domain=["Kelly", "Flat"],
                                    range=[[1, 0], [5, 5]]),
                    title="Strategy",
                ),
                tooltip=["trader_name", "timestamp:T", "portfolio_value:Q", "pnl:Q"],
            )
            .properties(height=350)
            .interactive()
        )
        st.altair_chart(trader_chart, use_container_width=True)

# --------------------------------------------------------------------------- #
# 3. Fair Value Drift Monitor (conviction loop early warning)
# --------------------------------------------------------------------------- #
st.header("Fair Value Drift Monitor")

fv_query = (
    f"SELECT trader_name, market_name, market_id, fair_value, market_price, timestamp "
    f"FROM decisions "
    f"WHERE trader_name IN ({trader_filter})"
    + (f" AND timestamp > '{cutoff}'" if cutoff else "")
    + " ORDER BY timestamp"
)
fv_df = pd.read_sql_query(fv_query, conn)

if fv_df.empty:
    st.info("No decisions yet to monitor.")
else:
    # Get latest FV per (trader, market)
    latest_fv = fv_df.groupby(["trader_name", "market_name", "market_id"]).agg(
        current_fv=("fair_value", "last"),
        current_mkt=("market_price", "last"),
        n_decisions=("fair_value", "count"),
    ).reset_index()

    # Get last 5 FVs as trend
    def fv_trend(group):
        vals = group["fair_value"].tail(5).tolist()
        return " -> ".join(f"{v:.2f}" for v in vals)

    trends = fv_df.groupby(["trader_name", "market_name"]).apply(
        fv_trend, include_groups=False
    ).reset_index(name="fv_trend")

    latest_fv = latest_fv.merge(trends, on=["trader_name", "market_name"], how="left")

    # Flag extreme FVs
    latest_fv["warning"] = latest_fv["current_fv"].apply(
        lambda fv: "EXTREME" if fv > 0.85 or fv < 0.15 else ""
    )
    latest_fv["strategy"] = latest_fv["trader_name"].apply(extract_strategy)
    latest_fv["edge"] = (latest_fv["current_fv"] - latest_fv["current_mkt"]).abs()

    # Show warnings first
    warnings = latest_fv[latest_fv["warning"] != ""]
    if not warnings.empty:
        st.warning(f"{len(warnings)} extreme fair value(s) detected — possible conviction loop")

    display_fv = latest_fv[["trader_name", "market_name", "current_fv", "current_mkt",
                            "edge", "fv_trend", "n_decisions", "warning"]].copy()
    display_fv.columns = ["Trader", "Market", "FV", "Mkt Price", "Edge", "FV Trend", "Decisions", "Warning"]
    display_fv = display_fv.sort_values(["Warning", "Edge"], ascending=[True, False])
    display_fv["FV"] = display_fv["FV"].apply(lambda x: f"{x:.2f}")
    display_fv["Mkt Price"] = display_fv["Mkt Price"].apply(lambda x: f"{x:.2f}")
    display_fv["Edge"] = display_fv["Edge"].apply(lambda x: f"{x:.3f}")

    st.dataframe(display_fv, use_container_width=True, hide_index=True)

# --------------------------------------------------------------------------- #
# 4. Portfolio Summary
# --------------------------------------------------------------------------- #
st.header("Portfolio Summary")

snap_query = f"""
    SELECT trader_name, balance, portfolio_value, pnl, positions, timestamp
    FROM portfolio_snapshots
    WHERE id IN (
        SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name
    )
    AND trader_name IN ({trader_filter})
    ORDER BY trader_name
"""
snap_df = pd.read_sql_query(snap_query, conn)

if snap_df.empty:
    st.info("No portfolio snapshots yet. Bots may not have started.")
else:
    snap_df["strategy"] = snap_df["trader_name"].apply(extract_strategy)
    display_df = snap_df[["trader_name", "strategy", "balance", "portfolio_value", "pnl", "timestamp"]].copy()
    display_df.columns = ["Trader", "Strategy", "Cash", "Portfolio Value", "PnL", "Last Updated"]
    display_df["Cash"] = display_df["Cash"].apply(lambda x: f"${x:.2f}")
    display_df["Portfolio Value"] = display_df["Portfolio Value"].apply(lambda x: f"${x:.2f}")
    display_df["PnL"] = display_df["PnL"].apply(lambda x: f"${x:+.2f}")
    st.dataframe(display_df, use_container_width=True, hide_index=True)

# --------------------------------------------------------------------------- #
# 5. Recent Decisions
# --------------------------------------------------------------------------- #
st.header("Recent Decisions")

if cutoff:
    dec_query = f"""
        SELECT trader_name, market_name, fair_value, market_price, orders,
               motivation, analysis, llm_duration_s, timestamp, balance, article_urls
        FROM decisions
        WHERE trader_name IN ({trader_filter}) AND timestamp > '{cutoff}'
        ORDER BY id DESC LIMIT 50
    """
else:
    dec_query = f"""
        SELECT trader_name, market_name, fair_value, market_price, orders,
               motivation, analysis, llm_duration_s, timestamp, balance, article_urls
        FROM decisions
        WHERE trader_name IN ({trader_filter})
        ORDER BY id DESC LIMIT 50
    """

dec_df = pd.read_sql_query(dec_query, conn)

if dec_df.empty:
    st.info("No decisions yet. Waiting for news articles to trigger LLM calls.")
else:
    for _, row in dec_df.iterrows():
        orders = json.loads(row["orders"]) if row["orders"] else []
        edge = abs(row["fair_value"] - row["market_price"])
        orders_str = ", ".join(
            f"{o['side']} {o['qty']}@${o['price']:.2f}" for o in orders
        ) if orders else "HOLD"

        strategy = extract_strategy(row["trader_name"])
        ts = row["timestamp"][:16] if row["timestamp"] else ""
        header = f"**{row['trader_name']}** | {row['market_name'][:50]} | FV={row['fair_value']:.2f} vs Mkt={row['market_price']:.2f} (edge={edge:.2f}) | {orders_str}"

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

if cutoff:
    art_query = f"""
        SELECT title, source, url, fetched_at, matched_market_ids
        FROM articles
        WHERE fetched_at > '{cutoff}'
        ORDER BY id DESC LIMIT 30
    """
else:
    art_query = "SELECT title, source, url, fetched_at, matched_market_ids FROM articles ORDER BY id DESC LIMIT 30"

art_df = pd.read_sql_query(art_query, conn)

if art_df.empty:
    st.info("No articles ingested yet.")
else:
    for _, row in art_df.iterrows():
        market_ids = json.loads(row["matched_market_ids"]) if row["matched_market_ids"] else []
        ts = row["fetched_at"][:16] if row["fetched_at"] else ""
        title = row["title"] or "(no title)"
        url = row["url"] or ""
        if url:
            st.markdown(f"- **{ts}** [{row['source']}] [{title}]({url}) -> {len(market_ids)} market(s)")
        else:
            st.markdown(f"- **{ts}** [{row['source']}] {title} -> {len(market_ids)} market(s)")

# --------------------------------------------------------------------------- #
# 7. Token Usage / Cost
# --------------------------------------------------------------------------- #
st.header("LLM Cost Tracker")

has_token_table = conn.execute(
    "SELECT name FROM sqlite_master WHERE type='table' AND name='token_usage'"
).fetchone()

if has_token_table:
    token_query = f"""
        SELECT trader_name,
               COUNT(*) as calls,
               SUM(prompt_tokens) as total_prompt,
               SUM(completion_tokens) as total_completion,
               AVG(duration_s) as avg_latency_s
        FROM token_usage
        {f"WHERE timestamp > '{cutoff}'" if cutoff else ""}
        GROUP BY trader_name
    """
    token_df = pd.read_sql_query(token_query, conn)

    if not token_df.empty:
        token_df["est_cost"] = (
            token_df["total_prompt"] * 0.70 / 1_000_000
            + token_df["total_completion"] * 0.70 / 1_000_000
        )
        token_df.columns = ["Trader", "Calls", "Prompt Tokens", "Completion Tokens", "Avg Latency (s)", "Est. Cost ($)"]
        token_df["Est. Cost ($)"] = token_df["Est. Cost ($)"].apply(lambda x: f"${x:.4f}")
        token_df["Avg Latency (s)"] = token_df["Avg Latency (s)"].apply(lambda x: f"{x:.1f}")
        st.dataframe(token_df, use_container_width=True, hide_index=True)

        total_cost_row = conn.execute(
            f"SELECT SUM(prompt_tokens), SUM(completion_tokens) FROM token_usage"
            + (f" WHERE timestamp > '{cutoff}'" if cutoff else "")
        ).fetchone()
        if total_cost_row[0]:
            total_cost = (total_cost_row[0] + total_cost_row[1]) * 0.70 / 1_000_000
            st.metric("Total Estimated Cost", f"${total_cost:.4f}")
    else:
        st.info("No token usage data yet.")
else:
    st.info("Token usage tracking not available (older DB schema).")

# --------------------------------------------------------------------------- #
# 8. Stats
# --------------------------------------------------------------------------- #
st.header("Stats")
col1, col2, col3 = st.columns(3)

total_decisions = conn.execute("SELECT COUNT(*) FROM decisions").fetchone()[0]
total_articles = conn.execute("SELECT COUNT(*) FROM articles").fetchone()[0]
total_snapshots = conn.execute("SELECT COUNT(*) FROM portfolio_snapshots").fetchone()[0]

col1.metric("Total Decisions", total_decisions)
col2.metric("Total Articles", total_articles)
col3.metric("Total Snapshots", total_snapshots)

conn.close()
