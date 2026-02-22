"""News dataset explorer for Iran strike market simulation."""

import json
import statistics
from collections import Counter
from datetime import datetime, timedelta
from pathlib import Path

import pandas as pd
import streamlit as st

DATA_PATH = Path(__file__).parent.parent / "datasets" / "iran_news_raw.json"
PHASE1_DIR = Path(__file__).parent / "tmp"
RUNS_DIR = Path(__file__).parent / "runs"

# ── Bot persona definitions ──
# Each bot has a name, description, and filter criteria.
# Add new bots here and they'll automatically appear as tabs.

BOT_PERSONAS = {
    "american_trader": {
        "name": "American Trader",
        "description": "Reads major US political/news outlets + UK mainstream with US audience. "
                       "Right-leaning skew reflects what US retail traders actually consume.",
        "sources": [
            # Tier 1: Major US national
            "yahoo.com", "nypost.com", "foxnews.com", "washingtonexaminer.com",
            "newsweek.com", "cnbc.com", "cbsnews.com", "abcnews.go.com",
            "bostonglobe.com", "forbes.com", "pbs.org", "cnn.com", "edition.cnn.com",
            "us.cnn.com", "nbcnews.com", "time.com", "latimes.com", "upi.com",
            "chicagotribune.com", "npr.org", "theatlantic.com", "abcnews.com",
            "politico.eu",
            # Tier 2: US political/opinion
            "breitbart.com", "aol.com", "foreignpolicy.com", "dailycaller.com",
            "csmonitor.com", "seattletimes.com", "baltimoresun.com",
            "dallasnews.com", "denverpost.com",
            # Tier 3: UK outlets with major US readership
            "dailymail.co.uk", "independent.co.uk", "theguardian.com",
            "bbc.co.uk", "bbc.com",
        ],
    },
}


@st.cache_data
def load_data() -> pd.DataFrame:
    with open(DATA_PATH) as f:
        raw = json.load(f)
    # Raw file has articles nested inside chunks
    articles = []
    for chunk in raw["chunks"]:
        articles.extend(chunk["articles"])
    df = pd.DataFrame(articles)
    df["dt"] = pd.to_datetime(df["timestamp"], format="%Y%m%dT%H%M%SZ")
    df["date"] = df["dt"].dt.date
    df["hour"] = df["dt"].dt.hour
    return df


@st.cache_data
def load_accepted_articles(bot_key: str) -> pd.DataFrame | None:
    """Load Phase 1 results for a bot. Looks for *_phase1_results.json files."""
    results = []
    if not PHASE1_DIR.exists():
        return None
    for f in sorted(PHASE1_DIR.glob("*_phase1_results.json")):
        with open(f) as fh:
            data = json.load(fh)
        for art in data.get("results", []):
            art["_file"] = f.name
            results.append(art)
    if not results:
        return None
    df = pd.DataFrame(results)
    df = df[df["phase1"].isin(["YES", "SKIP"])].copy()
    df["dt"] = pd.to_datetime(df["timestamp"], format="%Y%m%dT%H%M%SZ")
    df["date"] = df["dt"].dt.date
    # Load full text if available
    fulltext_path = PHASE1_DIR / "article_texts.json"
    if fulltext_path.exists():
        with open(fulltext_path) as fh:
            texts = json.load(fh)
        df["full_text"] = df["url"].map(texts)
    else:
        df["full_text"] = None
    return df


def render_bot_tab(df: pd.DataFrame, bot_key: str, persona: dict):
    """Render a bot persona tab with filters, stats, and article list."""
    bot_df = df[df["source"].isin(persona["sources"])].copy()

    st.markdown(f"*{persona['description']}*")

    bot_sub = st.radio(
        "View",
        ["General Stats", "Accepted Articles"],
        horizontal=True,
        key=f"{bot_key}_view",
    )

    if bot_sub == "Accepted Articles":
        accepted = load_accepted_articles(bot_key)
        if accepted is None or accepted.empty:
            st.info("No Phase 1 results yet. Run the relevance filter to populate.")
        else:
            yes_count = (accepted["phase1"] == "YES").sum()
            skip_count = (accepted["phase1"] == "SKIP").sum()
            st.caption(f"**{yes_count}** articles accepted, **{skip_count}** skipped (fetch failed)")
            for _, row in accepted.sort_values("dt").iterrows():
                time_str = row["dt"].strftime("%Y-%m-%d %H:%M")
                is_skip = row["phase1"] == "SKIP"
                status = " SKIP" if is_skip else ""
                label = f"{time_str} — {row['source']} — {row['title'][:70]}{status}"
                full_text = row.get("full_text")
                with st.expander(label):
                    if is_skip:
                        st.warning("Article could not be fetched (404 or blocked). Bot will skip this article.")
                    st.markdown(
                        f"**{time_str}** · [{row['title']}]({row['url']})  \n<small>{row['source']}</small>",
                        unsafe_allow_html=True,
                    )
                    if full_text:
                        st.markdown("---")
                        st.markdown(full_text)
                    elif not is_skip:
                        st.caption("Full text not yet fetched.")
        return

    # Metrics
    c1, c2, c3, c4 = st.columns(4)
    c1.metric("Total articles", f"{len(bot_df):,}")
    c2.metric("Sources matched", bot_df["source"].nunique())
    days = bot_df["date"].nunique()
    c3.metric("Days covered", days)
    c4.metric("Avg/day", f"{len(bot_df) / max(days, 1):.1f}")

    # Articles per day
    st.subheader("Articles per day")
    daily = bot_df.groupby("date").size().reset_index(name="count")
    st.bar_chart(daily, x="date", y="count", height=250)

    # Source breakdown
    col_l, col_r = st.columns(2)
    with col_l:
        st.subheader("Sources")
        src = bot_df["source"].value_counts().reset_index()
        src.columns = ["source", "articles"]
        st.dataframe(src, hide_index=True, height=400, use_container_width=True)
    with col_r:
        st.subheader("Source filter list")
        st.caption("These sources define this bot's news feed:")
        st.code("\n".join(sorted(persona["sources"])), language=None)

    # Daily article browser
    st.subheader("Articles by day")
    all_dates = sorted(bot_df["date"].unique())
    if not all_dates:
        st.warning("No articles for this bot.")
        return

    selected_date = st.select_slider(
        "Select date",
        options=all_dates,
        value=all_dates[len(all_dates) // 2],
        key=f"{bot_key}_date",
    )

    day_df = bot_df[bot_df["date"] == selected_date].sort_values("dt")
    st.caption(f"**{selected_date}** — {len(day_df)} articles")

    for _, row in day_df.iterrows():
        time_str = row["dt"].strftime("%H:%M")
        st.markdown(
            f"**{time_str}** · [{row['title']}]({row['url']})  \n"
            f"<small>{row['source']} ({row['sourcecountry']})</small>",
            unsafe_allow_html=True,
        )
        st.markdown("---")



@st.cache_data
def load_sim_runs() -> list[tuple[str, dict]]:
    """Load all simulation run JSON files, newest first."""
    if not RUNS_DIR.exists():
        return []
    runs = []
    for f in sorted(RUNS_DIR.glob("*.json"), reverse=True):
        with open(f) as fh:
            runs.append((f.stem, json.load(fh)))
    return runs


def _build_block_times(blocks, trade_log):
    """Build sim_time for each block from block-level sim_time, trade_log, or interpolation."""
    known = {}

    # 1. Prefer per-block sim_time (available in newer runs)
    for b in blocks:
        st_str = b.get("sim_time")
        if st_str:
            known[b["height"]] = datetime.fromisoformat(st_str)

    if known:
        return known

    # 2. Fall back to trade_log sim_times + interpolation (older runs)
    for t in trade_log:
        bh = t.get("block_height", -1)
        st_str = t.get("sim_time")
        if bh >= 0 and st_str:
            known[bh] = datetime.fromisoformat(st_str)

    # 3. Last resort: server wall-clock timestamps
    if not known:
        for b in blocks:
            ts = b.get("timestamp_ms")
            if ts:
                known[b["height"]] = datetime.fromtimestamp(ts / 1000)

    if not known:
        return {}

    # Interpolate: assume uniform block spacing between known points
    heights = [b["height"] for b in blocks]
    sorted_known = sorted(known.items())

    result = dict(known)
    for i in range(len(sorted_known) - 1):
        h1, t1 = sorted_known[i]
        h2, t2 = sorted_known[i + 1]
        for h in heights:
            if h1 < h < h2 and h not in result:
                frac = (h - h1) / (h2 - h1)
                result[h] = t1 + (t2 - t1) * frac

    # Extrapolate before first and after last known point
    if len(sorted_known) >= 2:
        h1, t1 = sorted_known[0]
        h2, t2 = sorted_known[1]
        per_block = (t2 - t1) / (h2 - h1) if h2 != h1 else timedelta(seconds=5)
        for h in heights:
            if h not in result:
                result[h] = t1 + per_block * (h - h1)

    return result


def _format_duration(td: timedelta) -> str:
    total_secs = int(td.total_seconds())
    days, rem = divmod(total_secs, 86400)
    hours, rem = divmod(rem, 3600)
    mins, _ = divmod(rem, 60)
    parts = []
    if days:
        parts.append(f"{days}d")
    if hours:
        parts.append(f"{hours}h")
    if mins:
        parts.append(f"{mins}m")
    return " ".join(parts) or "< 1m"


def render_simulation_tab():
    """Render the simulation replay tab."""
    runs = load_sim_runs()
    if not runs:
        st.info("No simulation runs yet. Run `uv run python -m iran.runner` to generate data.")
        return

    # Run selector
    run_names = [name for name, _ in runs]
    selected_name = st.selectbox("Select run", run_names, index=0)
    run_data = next(d for n, d in runs if n == selected_name)

    blocks = run_data["blocks"]
    leaderboard = run_data["leaderboard"]
    config = run_data["meta"]["config"]

    # Backward compat: old runs have "trade_log" (list), new have "trade_logs" (dict)
    trade_logs: dict[str, list] = run_data.get("trade_logs", {})
    if not trade_logs and "trade_log" in run_data:
        trade_logs = {"AmericanTrader": run_data["trade_log"]}
    # Flat list of all entries for summary stats / chart
    all_trade_entries = [t for entries in trade_logs.values() for t in entries]

    block_times = _build_block_times(blocks, all_trade_entries)

    # ── Classify leaderboard entries ──
    trader_names = set(trade_logs.keys())
    trader_rows = [r for r in leaderboard if r["name"] in trader_names]
    mm_row = next((r for r in leaderboard if r["name"] == "MM"), None)
    noise_rows = [r for r in leaderboard if r["name"].startswith("Noise")]
    noise_pnls = sorted([r["pnl"] for r in noise_rows]) if noise_rows else []

    # ═══════════ SUMMARY ═══════════
    st.subheader("Summary")

    # General
    st.markdown("**General**")
    # Server volume_nanos counts both sides of each fill; halve for single-counted volume
    total_vol = sum(b["volume_nanos"] for b in blocks) // 2
    first_h = blocks[0]["height"] if blocks else 0
    last_h = blocks[-1]["height"] if blocks else 0
    t_first = block_times.get(first_h)
    t_last = block_times.get(last_h)

    total_welfare = sum(b.get("welfare_nanos", 0) for b in blocks)
    total_fills = sum(b.get("orders_filled", 0) for b in blocks)
    has_welfare = any(b.get("welfare_nanos") for b in blocks)

    c1, c2, c3, c4, c5 = st.columns(5)
    c1.metric("Blocks", len(blocks))
    c2.metric("Total Volume", f"${total_vol / 1e9:,.0f}")
    if has_welfare:
        c3.metric("Total Welfare", f"${total_welfare / 1e9:,.0f}")
        c4.metric("Total Fills", f"{total_fills:,}")
    if t_first and t_last:
        duration = t_last - t_first
        c5.metric("Duration", _format_duration(duration))
        st.caption(f"Period: {t_first:%b %d %H:%M} – {t_last:%H:%M} ({_format_duration(duration)})")
    elif not has_welfare:
        c3.metric("Duration", "N/A")
        c4.metric("Period", f"Block {first_h} – {last_h}")

    # LLM Traders
    st.markdown("**LLM Traders**")
    trades_with_orders = [t for t in all_trade_entries if t["orders"]]
    has_fill_data = any(b_f.get("trader_fills") for b_f in blocks)
    filled_vol_nanos = 0
    if has_fill_data:
        for b_fill in blocks:
            for f in b_fill.get("trader_fills", []):
                filled_vol_nanos += f["fill_qty"] * int(f["fill_price"] * 1e9)

    cols = st.columns(3 + len(trader_rows))
    cols[0].metric("Articles Processed", f"{len(all_trade_entries)} ({len(trade_logs)} traders)")
    cols[1].metric("Trades Executed", len(trades_with_orders))
    cols[2].metric("Volume Filled", f"${filled_vol_nanos / 1e9:,.0f}" if has_fill_data else "N/A (rerun needed)")
    for i, r in enumerate(trader_rows):
        cols[3 + i].metric(f"{r['name']} PnL", f"${r['pnl']:+.2f}")

    # Others
    st.markdown("**Market Maker & Noise**")
    c1, c2, c3, c4 = st.columns(4)
    c1.metric("MM PnL", f"${mm_row['pnl']:+.2f}" if mm_row else "N/A")
    if noise_pnls:
        c2.metric("Noise PnL (min)", f"${noise_pnls[0]:+.2f}")
        c3.metric("Noise PnL (max)", f"${noise_pnls[-1]:+.2f}")
        c4.metric("Noise PnL (median)", f"${statistics.median(noise_pnls):+.2f}")

    # Config line
    model_short = config.get("model_name", "?").split("/")[-1]
    trader_list = ", ".join(trade_logs.keys()) or "none"
    st.caption(
        f"LLM traders: {trader_list} ({model_short}) · "
        f"Compression: {config.get('compression_ratio', '?')}x · "
        f"Noise: {config.get('noise_count', '?')} bots · "
        f"MM risk: {config.get('mm_risk_fraction', config.get('mm_budget', '?'))}"
    )

    # ═══════════ PRICE CHART ═══════════
    st.subheader("Price + Trader Activity")

    # Build price data — include all blocks, forward-fill nulls
    price_rows = []
    last_price = None
    for b in blocks:
        p = b["yes_price"]
        if p is not None:
            last_price = p
        t = block_times.get(b["height"])
        time_label = t.strftime("%b %d %H:%M") if t else ""
        price_rows.append({
            "block": b["height"],
            "time": time_label,
            "yes_price": last_price,
        })
    price_df = pd.DataFrame(price_rows)
    price_df = price_df.dropna(subset=["yes_price"])

    # Build x-axis label: "block (HH:MM)"
    price_df["x_label"] = price_df.apply(
        lambda r: f"{int(r['block'])} ({r['time']})" if r["time"] else str(int(r["block"])),
        axis=1,
    )

    # Trader events — use all_trade_entries as source of truth (orders may land 1 block later)
    trader_events = []
    for tname, tlog in trade_logs.items():
        for tl in tlog:
            bh = tl.get("block_height", -1)
            t = block_times.get(bh)
            time_str = t.strftime("%b %d %H:%M") if t else ""
            orders_str = ", ".join(tl["orders"]) if tl["orders"] else "NO TRADE"

            # Find the actual block where orders appeared (may be bh or bh+1)
            order_block = bh
            if tl["orders"]:
                for bk in blocks:
                    if bh <= bk["height"] <= bh + 2 and bk["trader_orders"]:
                        order_block = bk["height"]
                        break

            # Collect fills within TTL window from order submission
            ttl_end = order_block + 2  # inclusive
            nearby_fills = []
            for bk in blocks:
                if order_block <= bk["height"] <= ttl_end:
                    for f in bk.get("trader_fills", []):
                        fsrc = f.get("source", "Trader")
                        if fsrc in (tname, "Trader"):
                            nearby_fills.append(f)
            if nearby_fills:
                total_filled = sum(f["fill_qty"] for f in nearby_fills)
                avg_price = sum(f["fill_price"] * f["fill_qty"] for f in nearby_fills) / total_filled if total_filled else 0
                fill_str = f"{total_filled} filled @ {avg_price:.4f}"
            elif tl["orders"]:
                fill_str = "no fills"
            else:
                fill_str = ""

            # Use LLM block for chart position (that's when the decision was made)
            llm_block_data = next((b for b in blocks if b["height"] == bh), None)
            yes_price = llm_block_data["yes_price"] if llm_block_data and llm_block_data["yes_price"] is not None else 0

            trader_events.append({
                "block": bh,
                "trader": tname,
                "time": time_str,
                "yes_price": yes_price,
                "probability": tl["probability"],
                "conviction": tl["conviction"],
                "orders": orders_str,
                "fills": fill_str,
            })
    trader_df = pd.DataFrame(trader_events) if trader_events else None

    try:
        import altair as alt

        # Use time as the X axis directly
        # Add a sortable time column for blocks that have time, using block as fallback
        price_df["x_time"] = price_df["time"].replace("", None)
        # For axis: use block:O (ordinal) with time labels where available
        # Build lookup: block -> display label
        block_time_map = {}
        for _, r in price_df.iterrows():
            block_time_map[int(r["block"])] = r["time"] if r["time"] else str(int(r["block"]))

        # Pick ~10 tick positions
        tick_blocks = [int(r["block"]) for _, r in price_df.iterrows() if r["time"]]
        if not tick_blocks:
            tick_blocks = price_df["block"].tolist()
        step = max(1, len(tick_blocks) // 10)
        tick_values = tick_blocks[::step]

        # Build condition expression for axis labels
        conditions = " : ".join(
            f"datum.value == {bl} ? '{block_time_map.get(bl, bl)}'" for bl in tick_values
        )
        label_expr = f"{conditions} : ''" if conditions else "datum.value"

        base = alt.Chart(price_df).mark_line(color="#4A90D9", strokeWidth=2).encode(
            x=alt.X("block:Q", title="", axis=alt.Axis(
                values=tick_values,
                labelExpr=label_expr,
                labelAngle=315,
                labelOverlap=False,
                labelPadding=5,
            )),
            y=alt.Y("yes_price:Q", title="YES Price", scale=alt.Scale(zero=False)),
            tooltip=["block:Q", "time:N", alt.Tooltip("yes_price:Q", format=".4f")],
        )

        layers = [base]

        if trader_df is not None and not trader_df.empty:
            has_multi = trader_df["trader"].nunique() > 1 if "trader" in trader_df.columns else False
            color_enc = alt.Color("trader:N", title="Trader") if has_multi else alt.value("#E74C3C")

            # LLM probability dots
            prob_dots = alt.Chart(trader_df).mark_point(
                size=120, filled=True,
            ).encode(
                x="block:Q",
                y=alt.Y("probability:Q"),
                color=color_enc,
                tooltip=[
                    alt.Tooltip("block:Q", title="Block"),
                    alt.Tooltip("trader:N", title="Trader"),
                    alt.Tooltip("time:N", title="Sim Time"),
                    alt.Tooltip("probability:Q", title="LLM P", format=".2f"),
                    alt.Tooltip("conviction:N", title="Conviction"),
                    alt.Tooltip("orders:N", title="Orders"),
                    alt.Tooltip("fills:N", title="Fills"),
                ],
            )
            rules = alt.Chart(trader_df).mark_rule(
                strokeDash=[4, 4], opacity=0.4,
            ).encode(
                x="block:Q",
                color=color_enc,
            )
            labels = alt.Chart(trader_df).mark_text(
                align="left", dx=5, dy=-10, fontSize=10,
            ).encode(
                x="block:Q",
                y="probability:Q",
                color=color_enc,
                text=alt.Text("conviction:N"),
            )
            layers.extend([rules, prob_dots, labels])

        chart = alt.layer(*layers).properties(height=400, width="container")
        st.altair_chart(chart, use_container_width=True)
    except ImportError:
        st.line_chart(price_df.set_index("block")["yes_price"], height=350)

    # ═══════════ LEADERBOARD ═══════════
    st.subheader("Leaderboard")
    lb_df = pd.DataFrame(leaderboard)
    has_shares = "yes_shares" in lb_df.columns
    if has_shares:
        cols = ["name", "balance", "yes_shares", "no_shares", "position_value", "portfolio_value", "pnl"]
        names = ["Name", "Balance ($)", "YES Shares", "NO Shares", "Position Value ($)", "Portfolio ($)", "PnL ($)"]
    else:
        cols = ["name", "balance", "position_value", "portfolio_value", "pnl"]
        names = ["Name", "Balance ($)", "Position Value ($)", "Portfolio ($)", "PnL ($)"]
    lb_df = lb_df[[c for c in cols if c in lb_df.columns]]
    lb_df.columns = names[:len(lb_df.columns)]
    fmt = {
        "Balance ($)": "{:.2f}",
        "Position Value ($)": "{:.2f}",
        "Portfolio ($)": "{:.2f}",
        "PnL ($)": "{:+.2f}",
    }
    if has_shares:
        fmt["YES Shares"] = "{:,}"
        fmt["NO Shares"] = "{:,}"
    st.dataframe(
        lb_df.style.format(fmt).map(
            lambda v: "color: green" if v > 0 else "color: red" if v < 0 else "",
            subset=["PnL ($)"],
        ),
        hide_index=True,
        use_container_width=True,
    )

    # ═══════════ LLM TRADE LOG ═══════════
    st.subheader("LLM Decisions")

    llm_trader_names = list(trade_logs.keys()) or ["AmericanTrader"]
    selected_trader = st.selectbox("LLM Trader", llm_trader_names, key="llm_trader_select")
    trade_log = trade_logs.get(selected_trader, [])

    # Build price lookup: block_height -> yes_price (price the trader saw = previous block's clearing)
    price_at_block = {}
    prev_price = None
    for b in blocks:
        price_at_block[b["height"]] = prev_price  # price seen when deciding
        if b["yes_price"] is not None:
            prev_price = b["yes_price"]

    # Build fills lookup from blocks
    block_by_height = {b["height"]: b for b in blocks}

    for i, t in enumerate(trade_log, 1):
        bh = t.get("block_height", -1)
        order_block = bh
        orders_str = (", ".join(t["orders"]) or "NO TRADE").replace("$", "\\$")
        mkt_price = price_at_block.get(bh)
        mkt_str = f"mkt={mkt_price:.2f}" if mkt_price is not None else "mkt=?"

        # Collect per-fill data within TTL window (orders live for 3 blocks)
        # Only look for fills if this decision actually placed orders
        per_fill_items = []  # list of (fill_block, fill_qty, fill_price)
        if t["orders"]:
            ttl_end = order_block + 2  # inclusive
            for bk in blocks:
                if order_block <= bk["height"] <= ttl_end:
                    for f in bk.get("trader_fills", []):
                        # Filter to selected trader (new runs tag fills with source=trader name)
                        fsrc = f.get("source", "Trader")
                        if fsrc not in (selected_trader, "Trader"):
                            continue
                        per_fill_items.append((bk["height"], f["fill_qty"], f["fill_price"]))

        # Summary for header
        if per_fill_items:
            total_filled = sum(qty for _, qty, _ in per_fill_items)
            avg_price = sum(p * q for _, q, p in per_fill_items) / total_filled if total_filled else 0
            fill_str = f"{total_filled} filled @ {avg_price:.4f}"
        elif t["orders"]:
            fill_str = "no fills"
        else:
            fill_str = ""

        conv_color = {"HIGH": "red", "MEDIUM": "orange", "LOW": "gray"}.get(t["conviction"], "gray")
        header = (
            f"[{i}] Block {order_block} · "
            f"{mkt_str} → LLM P={t['probability']:.2f} · "
            f":{conv_color}[{t['conviction']}] · "
            f"{orders_str}"
        )
        with st.expander(header):
            # Timing info
            if order_block != bh:
                st.markdown(f"**LLM call initiated:** block {bh}")

            # Holdings at decision time (backward-compatible with older runs)
            bal = t.get("balance", 0)
            yp = t.get("yes_pos", 0)
            np_ = t.get("no_pos", 0)
            rp = t.get("risk_pct", 0)
            tp = t.get("target_pos", 0)
            blf = t.get("belief", 0)
            if bal or yp or np_:
                total_val = bal + yp * (mkt_price or 0) + np_ * (1 - (mkt_price or 0))
                holdings = f"**Holdings:** \\${bal:.2f} cash · YES {yp} · NO {np_} · total ~\\${total_val:.2f}"
                if blf > 0:
                    belief_side = "bullish" if blf > (mkt_price or 0.5) else "bearish"
                    holdings += f" | belief={blf:.3f} ({belief_side})"
                if rp > 0:
                    side = "YES" if blf > (mkt_price or 0) else "NO"
                    current = yp if side == "YES" else np_
                    gap = tp - current
                    sign = "+" if gap > 0 else ""
                    holdings += f" | kelly={rp:.1%} → target {side} {tp} (current {current}, {sign}{gap})"
                st.markdown(holdings)

            st.markdown(f"**Orders:** {orders_str}, block {order_block}" if t["orders"] else "**No trade** (edge too small or parse failure)")

            # Per-fill breakdown
            if per_fill_items:
                lines = ["**Fills:**"]
                for fill_block, fill_qty, fill_price in per_fill_items:
                    lines.append(f"- {fill_qty} filled @ {fill_price:.4f}, block {fill_block}")
                st.markdown("\n".join(lines))
            elif t["orders"]:
                st.markdown("**Fills:** no fills")

            # Then: article info
            st.markdown("---")
            st.markdown(f"**Article:** {t['article_source']} — {t['article_title']}")
            if t["motivation"]:
                st.info(t["motivation"])
            if t.get("llm_response"):
                st.markdown("**LLM Chain of Thought:**")
                st.text(t["llm_response"])

    # ═══════════ PER-BLOCK TABLE ═══════════
    st.subheader("Per-Block Activity")
    block_table = []
    for b in blocks:
        t = block_times.get(b["height"])
        # active_trader_orders includes TTL carry-over (newer runs);
        # fall back to len(trader_orders) for older run data.
        active_count = b.get("active_trader_orders", len(b["trader_orders"]))
        row = {
            "Block": b["height"],
            "Time": t.strftime("%H:%M:%S") if t else "",
            "Clearing Price": f"{b['yes_price']:.4f}" if b["yes_price"] is not None else "",
            "Volume ($)": f"{b['volume_nanos'] / 2e9:.2f}",
            "Welfare ($)": f"{b['welfare_nanos'] / 1e9:.2f}" if b.get("welfare_nanos") else "",
            "Fills": b.get("orders_filled", ""),
            "MM Orders": len(b["mm_orders"]),
            "Noise Orders": b["noise_order_count"],
            "Trader Orders": active_count,
            "LLM": "",
        }
        # trader_llm: list (new) or dict/None (old runs)
        raw_llm = b["trader_llm"]
        llm_list = raw_llm if isinstance(raw_llm, list) else ([raw_llm] if raw_llm else [])
        if llm_list:
            parts = []
            for l in llm_list:
                tag = f"[{l['trader']}] " if "trader" in l else ""
                parts.append(f"{tag}P={l['probability']:.2f} {l['conviction']}")
            row["LLM"] = " | ".join(parts)
        block_table.append(row)

    block_df = pd.DataFrame(block_table)
    st.dataframe(
        block_df.style.map(
            lambda v: "background-color: #fff3cd" if v else "",
            subset=["LLM"],
        ),
        hide_index=True,
        use_container_width=True,
        height=400,
    )

    # ═══════════ BLOCK INSPECTOR ═══════════
    st.subheader("Block Inspector")

    # ── Build live order tracker across all blocks ──
    # For each block, compute active orders = new submissions + carried-over (TTL=3, minus fills)
    # An "active order" is: (side, qty_remaining, price, submitted_block, source)
    def _parse_order(o: str):
        """Parse order strings in either format:
        Old: 'BuyYes 1071@0.28'
        New: 'BuyYes 1071 @ $0.28'
        """
        parts = o.split()
        if len(parts) < 2:
            return None
        side = parts[0]
        # New format: "BuyYes 1071 @ $0.28"
        if len(parts) >= 4 and parts[2] == "@":
            try:
                return side, int(parts[1]), float(parts[3].lstrip("$"))
            except (ValueError, IndexError):
                return None
        # Old format: "BuyYes 1071@0.28"
        if "@" in parts[1]:
            qty_price = parts[1].split("@")
            try:
                return side, int(qty_price[0]), float(qty_price[1])
            except (ValueError, IndexError):
                return None
        return None

    # Pre-compute active orders for every block
    # Each live order: {side, qty, price, submitted_block, source, original_qty}
    live_orders_by_block: dict[int, list[dict]] = {}  # post-fill (qty > 0)
    all_orders_by_block: dict[int, list[dict]] = {}   # pre+post fill (includes fully filled)
    carry_over: list[dict] = []  # orders still alive from previous blocks

    for b_iter in blocks:
        h = b_iter["height"]

        # 1. Expire orders past TTL (submitted 3+ blocks ago)
        carry_over = [o for o in carry_over if h - o["submitted_block"] < 3]

        # 2. Add new Trader/Noise orders BEFORE subtracting fills
        #    (same-block fills must reduce the new orders too)
        new_trader = []
        # Use per-trader detail when available (new runs), fall back to flat list
        detail = b_iter.get("trader_orders_detail")
        if detail:
            for entry in detail:
                parsed = _parse_order(entry["order"])
                if parsed:
                    side, qty, price = parsed
                    new_trader.append({
                        "side": side, "qty": qty, "price": price,
                        "submitted_block": h, "source": entry.get("trader", "Trader"),
                        "original_qty": qty, "filled": 0,
                    })
        else:
            for o_str in b_iter["trader_orders"]:
                parsed = _parse_order(o_str)
                if parsed:
                    side, qty, price = parsed
                    new_trader.append({
                        "side": side, "qty": qty, "price": price,
                        "submitted_block": h, "source": "Trader",
                        "original_qty": qty, "filled": 0,
                    })

        new_noise = []
        for o_str in b_iter["noise_orders"]:
            parsed = _parse_order(o_str)
            if parsed:
                side, qty, price = parsed
                new_noise.append({
                    "side": side, "qty": qty, "price": price,
                    "submitted_block": h, "source": "Noise",
                    "original_qty": qty, "filled": 0,
                })

        # Merge carry-over + new orders, then subtract fills
        all_fillable = carry_over + new_trader + new_noise

        def _delta_side(f):
            for d in f.get("position_deltas", []):
                if d["outcome"] == "YES" and d["delta"] > 0: return "BuyYes"
                if d["outcome"] == "YES" and d["delta"] < 0: return "SellYes"
                if d["outcome"] == "NO" and d["delta"] > 0: return "BuyNo"
                if d["outcome"] == "NO" and d["delta"] < 0: return "SellNo"
            return None

        for fill_key in ("trader_fills", "noise_fills"):
            for f in b_iter.get(fill_key, []):
                fside = _delta_side(f)
                fsource = f.get("source")
                remaining_fill = f["fill_qty"]
                for co in all_fillable:
                    if remaining_fill <= 0:
                        break
                    if co["qty"] > 0 and co["side"] == fside and co["source"] == fsource:
                        taken = min(co["qty"], remaining_fill)
                        co["qty"] -= taken
                        co["filled"] += taken
                        remaining_fill -= taken

        # MM re-submits every block via flash liquidity — no carry needed
        new_mm = []
        for o_str in b_iter["mm_orders"]:
            parsed = _parse_order(o_str)
            if parsed:
                side, qty, price = parsed
                new_mm.append({
                    "side": side, "qty": qty, "price": price,
                    "submitted_block": h, "source": "MM",
                    "original_qty": qty, "filled": 0,
                })

        # Subtract MM fills from MM orders
        for f in b_iter.get("mm_fills", []):
            fside = _delta_side(f)
            remaining_fill = f["fill_qty"]
            for co in new_mm:
                if remaining_fill <= 0:
                    break
                if co["qty"] > 0 and co["side"] == fside:
                    taken = min(co["qty"], remaining_fill)
                    co["qty"] -= taken
                    co["filled"] += taken
                    remaining_fill -= taken

        # Snapshot all orders (including fully filled) for Batch Summary matching
        all_orders_by_block[h] = [o.copy() for o in all_fillable] + [o.copy() for o in new_mm]

        # Active orders (post-fill, qty > 0) for orderbook chart
        all_active = [o.copy() for o in all_fillable if o["qty"] > 0] + new_mm
        live_orders_by_block[h] = all_active

        # Carry forward non-MM orders with remaining qty (TTL=3, expired at top of loop)
        # all_fillable already includes carry_over + new_trader + new_noise
        carry_over = [o for o in all_fillable if o["qty"] > 0 and o["source"] != "MM"]

    block_heights = [b["height"] for b in blocks]
    selected_block = st.selectbox("Select block", block_heights)
    b = next(b for b in blocks if b["height"] == selected_block)
    active_orders = live_orders_by_block.get(b["height"], [])
    submitted_orders = all_orders_by_block.get(b["height"], [])

    # ── Orderbook depth chart ──
    st.markdown("**Orderbook (YES-equivalent)**")
    st.caption(
        "BuyYes and SellNo are both bids for YES (bullish). "
        "BuyNo and SellYes are both asks on YES (bearish). "
        "Same directional bet, different mechanics: Buy uses cash, Sell uses inventory. "
        "Includes carried-over orders from previous blocks (TTL=3)."
    )

    # Build orderbook from ALL submitted orders (including fully filled)
    book_rows = []
    for o in submitted_orders:
        if o["original_qty"] <= 0:
            continue
        if o["side"] in ("BuyYes", "SellNo"):
            yes_equiv = 1.0 - o["price"] if o["side"] == "SellNo" else o["price"]
            side_label = "Bid"
        elif o["side"] in ("BuyNo", "SellYes"):
            yes_equiv = 1.0 - o["price"] if o["side"] == "BuyNo" else o["price"]
            side_label = "Ask"
        else:
            continue
        # Show filled portion dimmed, unfilled portion solid
        filled = o.get("filled", 0)
        remaining = o["original_qty"] - filled
        if filled > 0:
            book_rows.append({
                "price": round(yes_equiv, 4), "quantity": filled,
                "side": side_label, "source": o["source"], "status": "Filled",
            })
        if remaining > 0:
            book_rows.append({
                "price": round(yes_equiv, 4), "quantity": remaining,
                "side": side_label, "source": o["source"], "status": "Resting",
            })

    if book_rows:
        try:
            import altair as alt

            book_df = pd.DataFrame(book_rows)
            ob_chart = alt.Chart(book_df).mark_bar().encode(
                x=alt.X("price:Q", title="YES Price", scale=alt.Scale(zero=False)),
                y=alt.Y("quantity:Q", title="Quantity", stack=True),
                color=alt.Color("side:N", scale=alt.Scale(
                    domain=["Bid", "Ask"],
                    range=["#27AE60", "#E74C3C"],
                )),
                opacity=alt.condition(
                    alt.datum.status == "Filled",
                    alt.value(0.3),
                    alt.value(0.8),
                ),
                tooltip=[
                    "side:N", "source:N", "status:N",
                    alt.Tooltip("price:Q", format=".2f"), "quantity:Q",
                ],
            ).properties(height=250, width="container")

            if b["yes_price"] is not None:
                clearing_rule = alt.Chart(
                    pd.DataFrame([{"price": b["yes_price"]}])
                ).mark_rule(color="#4A90D9", strokeDash=[4, 4], strokeWidth=2).encode(
                    x="price:Q",
                )
                ob_chart = ob_chart + clearing_rule

            st.altair_chart(ob_chart, use_container_width=True)
        except ImportError:
            st.caption(f"{len(book_rows)} orders in book")
    else:
        st.caption("No active orders in this block")

    # ── Batch Summary: orders → fills → clearing price ──
    yes_price = b.get("yes_price")
    clearing_str = f"YES={yes_price:.4f}  NO={1 - yes_price:.4f}" if yes_price is not None else "no clearing"
    st.markdown(f"**Batch Summary** — clearing: {clearing_str}")

    # Core economics (available in newer runs)
    welfare_nanos = b.get("welfare_nanos")
    total_vol_nanos = b.get("total_volume_nanos")
    orders_filled = b.get("orders_filled")
    if welfare_nanos is not None:
        orders_submitted = len(submitted_orders)
        fill_rate = (orders_filled / orders_submitted * 100) if orders_submitted > 0 else 0
        ec1, ec2, ec3 = st.columns(3)
        ec1.metric("Welfare", f"${welfare_nanos / 1e9:,.2f}")
        ec2.metric("Volume", f"${total_vol_nanos / 1e9:,.2f}")
        ec3.metric("Fill Rate", f"{orders_filled}/{orders_submitted} ({fill_rate:.0f}%)")

    # Collect all fills indexed by source
    all_fills: list[dict] = []
    for key, fallback in [("trader_fills", "Trader"), ("mm_fills", "MM"), ("noise_fills", "Noise")]:
        for f in b.get(key, []):
            f.setdefault("source", fallback)
            all_fills.append(f)

    # Group fills by source for summary counts
    fills_by_source: dict[str, list[dict]] = {}
    for f in all_fills:
        fills_by_source.setdefault(f["source"], []).append(f)

    # Determine fill side from position deltas
    def _fill_side(f):
        for d in f.get("position_deltas", []):
            if d["outcome"] == "YES" and d["delta"] > 0: return "BuyYes"
            if d["outcome"] == "YES" and d["delta"] < 0: return "SellYes"
            if d["outcome"] == "NO" and d["delta"] > 0: return "BuyNo"
            if d["outcome"] == "NO" and d["delta"] < 0: return "SellNo"
        return None

    # Build dynamic source list: all unique sources from orders + fills
    all_sources = set(o["source"] for o in submitted_orders)
    all_sources.update(fills_by_source.keys())
    # Order: trader names first, then MM, then Noise
    fixed_order = {"MM": 900, "Noise": 999}
    sorted_sources = sorted(all_sources, key=lambda s: fixed_order.get(s, 0))

    # Per-source: combined orders + fills table
    for source in sorted_sources:
        orders = [o for o in submitted_orders if o["source"] == source]
        fills = fills_by_source.get(source, [])
        if not orders and not fills:
            continue

        # Group fills by side for matching to orders
        side_fills: dict[str, list[dict]] = {}
        for f in fills:
            side_fills.setdefault(_fill_side(f), []).append(f)

        # Build combined rows: each order paired with its fill (if any)
        rows = []
        for o in orders:
            matched = side_fills.get(o["side"], [])
            f = matched.pop(0) if matched else None
            row = {"Side": o["side"], "Qty": o["original_qty"], "Limit": f"${o['price']:.2f}"}
            if f:
                deltas = ", ".join(
                    f"{d['outcome']} {d['delta']:+d}" for d in f.get("position_deltas", [])
                )
                row["Filled"] = f["fill_qty"]
                row["Fill $"] = f"${f['fill_price']:.4f}"
                row["Δ Pos"] = deltas
            else:
                row["Filled"] = ""
                row["Fill $"] = ""
                row["Δ Pos"] = ""
            rows.append(row)

        # Leftover fills without a matched order
        for side, remaining in side_fills.items():
            for f in remaining:
                deltas = ", ".join(
                    f"{d['outcome']} {d['delta']:+d}" for d in f.get("position_deltas", [])
                )
                rows.append({
                    "Side": side or "?", "Qty": "", "Limit": "",
                    "Filled": f["fill_qty"],
                    "Fill $": f"${f['fill_price']:.4f}",
                    "Δ Pos": deltas,
                })

        # Sort: BuyYes → SellYes → BuyNo → SellNo, buys high→low, sells low→high
        side_order = {"BuyYes": 0, "SellYes": 1, "BuyNo": 2, "SellNo": 3}

        def _sort_key(r):
            s = side_order.get(r["Side"], 99)
            # Parse price for secondary sort
            limit = r.get("Limit", "")
            price = float(limit.lstrip("$")) if limit else 0
            # Buys: highest price first (negate); Sells: lowest price first
            is_buy = r["Side"].startswith("Buy")
            return (s, -price if is_buy else price)

        rows.sort(key=_sort_key)

        is_trader = source not in ("MM", "Noise")
        with st.expander(f"**{source}**: {len(orders)} orders → {len(fills)} fills", expanded=is_trader):
            if rows:
                st.dataframe(rows, use_container_width=True, hide_index=True)
            else:
                st.caption("No activity")



def main():
    st.set_page_config(page_title="Iran News Explorer", layout="wide")
    st.title("Iran Strike Market — News Dataset Explorer")

    df = load_data()

    # ── Sidebar filters ──
    st.sidebar.header("Filters")
    countries = sorted(df["sourcecountry"].unique())
    selected_countries = st.sidebar.multiselect("Countries", countries, default=[])
    languages = sorted(df["language"].unique())
    selected_languages = st.sidebar.multiselect("Languages", languages, default=[])
    sources = sorted(df["source"].unique())
    selected_sources = st.sidebar.multiselect("Sources", sources, default=[])

    filtered = df.copy()
    if selected_countries:
        filtered = filtered[filtered["sourcecountry"].isin(selected_countries)]
    if selected_languages:
        filtered = filtered[filtered["language"].isin(selected_languages)]
    if selected_sources:
        filtered = filtered[filtered["source"].isin(selected_sources)]

    # ── Tabs: Summary, Daily, Simulation, + one per bot persona ──
    bot_tab_names = [p["name"] for p in BOT_PERSONAS.values()]
    all_tabs = st.tabs(["Summary", "Daily Explorer", "Simulation"] + [f"Bot: {n}" for n in bot_tab_names])
    tab_summary = all_tabs[0]
    tab_daily = all_tabs[1]
    tab_simulation = all_tabs[2]
    bot_tabs = {k: all_tabs[i + 3] for i, k in enumerate(BOT_PERSONAS)}

    # ═══════════════════════════ SUMMARY ═══════════════════════════
    with tab_summary:
        # Top-level metrics
        c1, c2, c3, c4 = st.columns(4)
        c1.metric("Total articles", f"{len(filtered):,}")
        c2.metric("Unique sources", filtered["source"].nunique())
        c3.metric("Countries", filtered["sourcecountry"].nunique())
        c4.metric("Languages", filtered["language"].nunique())

        date_range = f"{filtered['date'].min()} → {filtered['date'].max()}"
        days = filtered["date"].nunique()
        st.caption(f"Period: {date_range}  ({days} days)")

        # ── Articles per day ──
        st.subheader("Articles per day")
        daily = filtered.groupby("date").size().reset_index(name="count")
        st.bar_chart(daily, x="date", y="count", height=300)

        # ── By country ──
        col_left, col_right = st.columns(2)

        with col_left:
            st.subheader("Top countries")
            country_counts = (
                filtered["sourcecountry"]
                .value_counts()
                .head(20)
                .reset_index()
            )
            country_counts.columns = ["country", "articles"]
            st.bar_chart(country_counts, x="country", y="articles", horizontal=True, height=500)

        with col_right:
            st.subheader("Top sources")
            source_counts = (
                filtered["source"]
                .value_counts()
                .head(20)
                .reset_index()
            )
            source_counts.columns = ["source", "articles"]
            st.bar_chart(source_counts, x="source", y="articles", horizontal=True, height=500)

        # ── By language ──
        st.subheader("Languages")
        lang_counts = (
            filtered["language"]
            .value_counts()
            .reset_index()
        )
        lang_counts.columns = ["language", "articles"]
        st.bar_chart(lang_counts, x="language", y="articles", height=300)

    # ═══════════════════════════ DAILY EXPLORER ═══════════════════════════
    with tab_daily:
        all_dates = sorted(filtered["date"].unique())
        if not all_dates:
            st.warning("No articles match the current filters.")
            return

        selected_date = st.select_slider(
            "Select date",
            options=all_dates,
            value=all_dates[0],
        )

        day_df = filtered[filtered["date"] == selected_date].sort_values("dt")

        st.subheader(f"{selected_date} — {len(day_df)} articles")

        # Hourly distribution for that day
        hourly = day_df.groupby("hour").size().reset_index(name="count")
        st.bar_chart(hourly, x="hour", y="count", height=200)

        # Country/source breakdown for day
        dc1, dc2 = st.columns(2)
        with dc1:
            st.caption("Countries this day")
            st.dataframe(
                day_df["sourcecountry"].value_counts().reset_index().rename(
                    columns={"index": "country", "sourcecountry": "country", "count": "articles"}
                ),
                hide_index=True,
                height=200,
            )
        with dc2:
            st.caption("Sources this day")
            st.dataframe(
                day_df["source"].value_counts().head(10).reset_index().rename(
                    columns={"index": "source", "source": "source", "count": "articles"}
                ),
                hide_index=True,
                height=200,
            )

        # Headlines
        st.subheader("Headlines")

        # Group by hour for readability
        for hour, group in day_df.groupby("hour"):
            with st.expander(f"{hour:02d}:00 — {len(group)} articles", expanded=(hour == day_df['hour'].iloc[0])):
                for _, row in group.iterrows():
                    time_str = row["dt"].strftime("%H:%M")
                    source_info = f"{row['source']} ({row['sourcecountry']}, {row['language']})"
                    st.markdown(
                        f"**{time_str}** · [{row['title']}]({row['url']})  \n"
                        f"<small>{source_info}</small>",
                        unsafe_allow_html=True,
                    )


    # ═══════════════════════════ SIMULATION ═══════════════════════════
    with tab_simulation:
        render_simulation_tab()

    # ═══════════════════════════ BOT PERSONAS ═══════════════════════════
    for bot_key, persona in BOT_PERSONAS.items():
        with bot_tabs[bot_key]:
            render_bot_tab(df, bot_key, persona)


if __name__ == "__main__":
    main()
