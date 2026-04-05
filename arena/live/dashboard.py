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
    time_clause = ""
    time_clause_snap = ""

# --------------------------------------------------------------------------- #
# Trader filter
# --------------------------------------------------------------------------- #
traders_df = pd.read_sql_query(
    "SELECT DISTINCT trader_name FROM decisions ORDER BY trader_name", conn
)
all_traders = traders_df["trader_name"].tolist() if not traders_df.empty else []

with st.sidebar:
    selected_traders = st.multiselect("Traders", all_traders, default=all_traders)

if not selected_traders:
    selected_traders = all_traders
trader_filter = ",".join(f"'{t}'" for t in selected_traders)

# --------------------------------------------------------------------------- #
# 1. Portfolio Summary
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
    display_df = snap_df[["trader_name", "balance", "portfolio_value", "pnl", "timestamp"]].copy()
    display_df.columns = ["Trader", "Cash", "Portfolio Value", "PnL", "Last Updated"]
    display_df["Cash"] = display_df["Cash"].apply(lambda x: f"${x:.2f}")
    display_df["Portfolio Value"] = display_df["Portfolio Value"].apply(lambda x: f"${x:.2f}")
    display_df["PnL"] = display_df["PnL"].apply(lambda x: f"${x:+.2f}")
    st.dataframe(display_df, use_container_width=True, hide_index=True)

# --------------------------------------------------------------------------- #
# 2. Recent Decisions
# --------------------------------------------------------------------------- #
st.header("Recent Decisions")

dec_query = f"""
    SELECT trader_name, market_name, fair_value, market_price, orders,
           motivation, analysis, llm_duration_s, timestamp, balance
    FROM decisions
    {time_clause.replace('WHERE', 'WHERE' if not selected_traders else f'WHERE trader_name IN ({trader_filter}) AND') if time_clause else f'WHERE trader_name IN ({trader_filter})'}
    ORDER BY id DESC
    LIMIT 50
"""
# Simplify the query
if time_clause:
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

        ts = row["timestamp"][:16] if row["timestamp"] else ""
        header = f"**{row['trader_name']}** | {row['market_name'][:50]} | FV={row['fair_value']:.2f} vs Mkt={row['market_price']:.2f} (edge={edge:.2f}) | {orders_str}"

        with st.expander(f"{ts} — {header}"):
            st.markdown(f"**Analysis:** {row['analysis']}")
            st.markdown(f"**Motivation:** {row['motivation']}")
            st.markdown(f"**LLM latency:** {row['llm_duration_s']:.1f}s | **Balance:** ${row['balance']:.2f}")

            # Show linked articles with clickable URLs
            article_urls = json.loads(row["article_urls"]) if row.get("article_urls") else []
            if article_urls:
                st.markdown("**Sources:**")
                for art in article_urls:
                    st.markdown(f"- [{art['title'][:80]}]({art['url']}) ({art['source']})")

# --------------------------------------------------------------------------- #
# 3. PnL Chart
# --------------------------------------------------------------------------- #
st.header("Portfolio Value Over Time")

if time_clause_snap:
    pnl_query = f"""
        SELECT trader_name, timestamp, portfolio_value, pnl
        FROM portfolio_snapshots
        WHERE trader_name IN ({trader_filter}) AND timestamp > '{cutoff}'
        ORDER BY timestamp
    """
else:
    pnl_query = f"""
        SELECT trader_name, timestamp, portfolio_value, pnl
        FROM portfolio_snapshots
        WHERE trader_name IN ({trader_filter})
        ORDER BY timestamp
    """

pnl_df = pd.read_sql_query(pnl_query, conn)

if pnl_df.empty:
    st.info("No data for PnL chart yet.")
else:
    pnl_df["timestamp"] = pd.to_datetime(pnl_df["timestamp"])
    chart = (
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
    st.altair_chart(chart, use_container_width=True)

# --------------------------------------------------------------------------- #
# 4. News Feed
# --------------------------------------------------------------------------- #
st.header("News Feed")

if time_clause:
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
            st.markdown(f"- **{ts}** [{row['source']}] [{title}]({url}) → {len(market_ids)} market(s)")
        else:
            st.markdown(f"- **{ts}** [{row['source']}] {title} → {len(market_ids)} market(s)")

# --------------------------------------------------------------------------- #
# 5. Token Usage / Cost
# --------------------------------------------------------------------------- #
st.header("LLM Cost Tracker")

# Check if token_usage table exists
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
        {f"WHERE timestamp > '{cutoff}'" if time_clause else ""}
        GROUP BY trader_name
    """
    token_df = pd.read_sql_query(token_query, conn)

    if not token_df.empty:
        # MiniMax M2.7 pricing: $0.70/M input, $0.70/M output (OpenRouter)
        token_df["est_cost"] = (
            token_df["total_prompt"] * 0.70 / 1_000_000
            + token_df["total_completion"] * 0.70 / 1_000_000
        )
        token_df.columns = ["Trader", "Calls", "Prompt Tokens", "Completion Tokens", "Avg Latency (s)", "Est. Cost ($)"]
        token_df["Est. Cost ($)"] = token_df["Est. Cost ($)"].apply(lambda x: f"${x:.4f}")
        token_df["Avg Latency (s)"] = token_df["Avg Latency (s)"].apply(lambda x: f"{x:.1f}")
        st.dataframe(token_df, use_container_width=True, hide_index=True)

        # Total cost
        total_cost_row = conn.execute(
            f"SELECT SUM(prompt_tokens), SUM(completion_tokens) FROM token_usage"
            + (f" WHERE timestamp > '{cutoff}'" if time_clause else "")
        ).fetchone()
        if total_cost_row[0]:
            total_cost = (total_cost_row[0] + total_cost_row[1]) * 0.70 / 1_000_000
            st.metric("Total Estimated Cost", f"${total_cost:.4f}")
    else:
        st.info("No token usage data yet.")
else:
    st.info("Token usage tracking not available (older DB schema).")

# --------------------------------------------------------------------------- #
# 6. Stats
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
