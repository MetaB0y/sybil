"""News dataset explorer for prediction market simulations.

Usage:
    cd arena && uv run streamlit run viz/news_explorer.py -- --market iran
"""

import importlib
import json
import statistics
import sys
from collections import Counter
from datetime import datetime, timedelta
from pathlib import Path

import pandas as pd
import streamlit as st


def _load_market_config(market_name: str):
    """Load a MarketConfig by name."""
    mod = importlib.import_module(f"markets.{market_name}")
    return mod.get_config()


# Parse --market from CLI args (streamlit passes args after --)
_market_name = "iran"  # default
if "--market" in sys.argv:
    idx = sys.argv.index("--market")
    if idx + 1 < len(sys.argv):
        _market_name = sys.argv[idx + 1]

_market_config = _load_market_config(_market_name)
DATASETS_DIR = _market_config.datasets_dir
PHASE1_DIR = _market_config.phase1_dir
RUNS_DIR = _market_config.runs_dir
BOT_PERSONAS = _market_config.personas

# Keep backward-compat module-level name for any remaining references

@st.cache_data
def load_data() -> pd.DataFrame:
    # Load all *_raw.json dataset files and deduplicate by URL
    articles = []
    for path in sorted(DATASETS_DIR.glob("*_raw.json")):
        with open(path) as f:
            raw = json.load(f)
        for chunk in raw["chunks"]:
            articles.extend(chunk["articles"])
    df = pd.DataFrame(articles)
    df = df.drop_duplicates(subset="url", keep="first")
    df["dt"] = pd.to_datetime(df["timestamp"], format="%Y%m%dT%H%M%SZ")
    df["date"] = df["dt"].dt.date
    df["hour"] = df["dt"].dt.hour
    return df


@st.cache_data
def load_accepted_articles(bot_key: str) -> pd.DataFrame | None:
    """Load Phase 1 results for a bot. Looks for *_phase1_results.json files."""
    # Support phase1_bot indirection (e.g. american_believer uses american_trader's results)
    phase1_key = BOT_PERSONAS.get(bot_key, {}).get("phase1_bot", bot_key)
    results = []
    if not PHASE1_DIR.exists():
        return None
    for f in sorted(PHASE1_DIR.glob(f"{phase1_key}_*_phase1_results.json")):
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
    if "full_text" not in df.columns:
        df["full_text"] = None
    return df


def _render_random_trader_tab(persona: dict):
    """Render the Random Sampler tab from pre-selected articles JSON."""
    st.markdown(f"*{persona['description']}*")

    articles_path = Path(__file__).parent / "tmp" / "random_trader_articles.json"
    if not articles_path.exists():
        st.warning("No random trader articles yet. Generate with the article selection script.")
        return

    data = json.loads(articles_path.read_text())
    st.caption(f"Seed: {data.get('seed', '?')} · {sum(d['selected'] for d in data['days'])} articles across {len(data['days'])} days")

    for day in data["days"]:
        date_str = day["date"]
        date_label = datetime.strptime(date_str, "%Y%m%d").strftime("%b %d, %Y")
        with st.expander(f"{date_label} — {day['selected']} articles (from {day['pool_size']} pool)", expanded=False):
            for a in day["articles"]:
                ts = datetime.strptime(a["timestamp"], "%Y%m%dT%H%M%SZ")
                time_str = ts.strftime("%H:%M")
                source = a.get("source", "?")
                title = a.get("title", "?")
                url = a.get("url", "")
                st.markdown(
                    f"**{time_str}** · [{title[:80]}]({url})  \n"
                    f"<small>{source}</small>",
                    unsafe_allow_html=True,
                )
                full_text = a.get("full_text", "")
                if full_text:
                    with st.expander("Full text", expanded=False):
                        st.markdown(full_text[:2000])


def render_bot_tab(df: pd.DataFrame, bot_key: str, persona: dict):
    """Render a bot persona tab with filters, stats, and article list."""
    if bot_key == "random_trader":
        return _render_random_trader_tab(persona)

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
    """Load all simulation run JSON files, newest first.

    Multi-day runs (sharing a run_id) are merged into a single entry:
    blocks are concatenated, trade_logs and leaderboard come from the last day.
    """
    if not RUNS_DIR.exists():
        return []

    # Load all files
    raw: list[tuple[str, dict]] = []
    for f in sorted(RUNS_DIR.glob("*.json")):
        with open(f) as fh:
            raw.append((f.stem, json.load(fh)))

    # Group by run_id (if present)
    groups: dict[str, list[tuple[str, dict]]] = {}  # run_id -> [(name, data), ...]
    standalone: list[tuple[str, dict]] = []
    for name, data in raw:
        rid = data.get("meta", {}).get("run_id")
        if rid:
            groups.setdefault(rid, []).append((name, data))
        else:
            standalone.append((name, data))

    runs: list[tuple[str, dict]] = []

    # Merge multi-day groups
    for rid, day_files in groups.items():
        if len(day_files) == 1:
            # Single file with run_id — no merge needed
            runs.append(day_files[0])
            continue

        # Sort by simulation_date so blocks concatenate in order
        day_files.sort(key=lambda x: x[1].get("meta", {}).get("simulation_date", ""))
        last_name, last_data = day_files[-1]

        # Concatenate blocks from all days
        all_blocks = []
        day_dates = []
        for _, d in day_files:
            all_blocks.extend(d.get("blocks", []))
            sd = d.get("meta", {}).get("simulation_date")
            if sd:
                day_dates.append(sd)

        # Build merged run: blocks from all days, trade_logs/leaderboard from last day
        merged = {
            "meta": {
                **last_data["meta"],
                "simulation_dates": day_dates,
            },
            "blocks": all_blocks,
            "trade_logs": last_data.get("trade_logs", {}),
            "trader_models": last_data.get("trader_models", {}),
            "leaderboard": last_data.get("leaderboard", []),
        }

        # Display name: "run_id (Jan 1-3)" or similar
        if day_dates:
            def _fmt_date(d):
                try:
                    dt = datetime.strptime(d, "%Y%m%d")
                    return dt.strftime("%b %-d")
                except ValueError:
                    return d
            date_label = f"{_fmt_date(day_dates[0])}–{_fmt_date(day_dates[-1])}"
            display_name = f"{rid} ({date_label}, {len(day_files)} days)"
        else:
            display_name = f"{rid} ({len(day_files)} days)"

        runs.append((display_name, merged))

    # Add standalone runs
    runs.extend(standalone)

    # Sort newest first
    runs.sort(key=lambda x: x[0], reverse=True)
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
        st.info(f"No simulation runs yet. Run `uv run python -m sim.runner --market {_market_name}` to generate data.")
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

    # ── Build live order tracker across all blocks (needed for welfare + block inspector) ──
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
        if len(parts) >= 4 and parts[2] == "@":
            try:
                return side, int(parts[1]), float(parts[3].lstrip("$"))
            except (ValueError, IndexError):
                return None
        if "@" in parts[1]:
            qty_price = parts[1].split("@")
            try:
                return side, int(qty_price[0]), float(qty_price[1])
            except (ValueError, IndexError):
                return None
        return None

    def _delta_side(f):
        for d in f.get("position_deltas", []):
            if d["outcome"] == "YES" and d["delta"] > 0: return "BuyYes"
            if d["outcome"] == "YES" and d["delta"] < 0: return "SellYes"
            if d["outcome"] == "NO" and d["delta"] > 0: return "BuyNo"
            if d["outcome"] == "NO" and d["delta"] < 0: return "SellNo"
        return None

    live_orders_by_block: dict[int, list[dict]] = {}
    all_orders_by_block: dict[int, list[dict]] = {}
    carry_over: list[dict] = []

    for b_iter in blocks:
        h = b_iter["height"]
        carry_over = [o for o in carry_over if h - o["submitted_block"] < 3]

        new_trader = []
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

        all_fillable = carry_over + new_trader + new_noise
        for o in all_fillable:
            o["filled_this_block"] = 0

        for fill_key in ("trader_fills", "noise_fills"):
            for f in b_iter.get(fill_key, []):
                fside = _delta_side(f)
                fsource = f.get("source")
                fill_price = f.get("fill_price", 0)
                remaining_fill = f["fill_qty"]
                for co in all_fillable:
                    if remaining_fill <= 0:
                        break
                    if co["qty"] > 0 and co["side"] == fside and co["source"] == fsource:
                        # Verify order limit is compatible with fill price
                        if fside in ("BuyYes", "BuyNo") and co["price"] < fill_price - 0.0001:
                            continue
                        if fside in ("SellYes", "SellNo") and co["price"] > fill_price + 0.0001:
                            continue
                        taken = min(co["qty"], remaining_fill)
                        co["qty"] -= taken
                        co["filled"] += taken
                        co["filled_this_block"] += taken
                        remaining_fill -= taken

        new_mm = []
        for o_str in b_iter["mm_orders"]:
            parsed = _parse_order(o_str)
            if parsed:
                side, qty, price = parsed
                new_mm.append({
                    "side": side, "qty": qty, "price": price,
                    "submitted_block": h, "source": "MM",
                    "original_qty": qty, "filled": 0, "filled_this_block": 0,
                })

        for f in b_iter.get("mm_fills", []):
            fside = _delta_side(f)
            fill_price = f.get("fill_price", 0)
            remaining_fill = f["fill_qty"]
            for co in new_mm:
                if remaining_fill <= 0:
                    break
                # Only match if order limit is compatible with fill price
                compatible = (
                    co["qty"] > 0 and co["side"] == fside
                    and (
                        (fside in ("BuyYes", "BuyNo") and co["price"] >= fill_price - 0.0001)
                        or (fside in ("SellYes", "SellNo") and co["price"] <= fill_price + 0.0001)
                    )
                )
                if compatible:
                    taken = min(co["qty"], remaining_fill)
                    co["qty"] -= taken
                    co["filled"] += taken
                    co["filled_this_block"] += taken
                    remaining_fill -= taken

        all_orders_by_block[h] = [o.copy() for o in all_fillable] + [o.copy() for o in new_mm]
        all_active = [o.copy() for o in all_fillable if o["qty"] > 0] + new_mm
        live_orders_by_block[h] = all_active
        carry_over = [o for o in all_fillable if o["qty"] > 0 and o["source"] != "MM"]

    # ── Compute per-source welfare ──
    # Buy welfare  = (limit - clearing) * qty  (buyer pays less than max willingness)
    # Sell welfare = (clearing - limit) * qty  (seller receives more than min acceptable)
    welfare_by_source: dict[str, float] = {}
    for b_w in blocks:
        yes_price = b_w.get("yes_price")
        if yes_price is None:
            continue
        for o in all_orders_by_block.get(b_w["height"], []):
            filled = o.get("filled_this_block", 0)
            if filled <= 0:
                continue
            if o["side"] == "BuyYes":
                w = (o["price"] - yes_price) * filled
            elif o["side"] == "BuyNo":
                w = (o["price"] - (1.0 - yes_price)) * filled
            elif o["side"] == "SellYes":
                w = (yes_price - o["price"]) * filled
            else:  # SellNo
                w = ((1.0 - yes_price) - o["price"]) * filled
            welfare_by_source[o["source"]] = welfare_by_source.get(o["source"], 0) + w

    mm_welfare = welfare_by_source.get("MM", 0)
    noise_welfare = welfare_by_source.get("Noise", 0)
    trader_welfare = sum(v for k, v in welfare_by_source.items() if k not in ("MM", "Noise"))

    # ═══════════ SUMMARY ═══════════
    st.subheader("Summary")

    # Subtitle line: period, traders, compression, noise
    first_h = blocks[0]["height"] if blocks else 0
    last_h = blocks[-1]["height"] if blocks else 0
    t_first = block_times.get(first_h)
    t_last = block_times.get(last_h)
    total_fills = sum(b.get("orders_filled", 0) for b in blocks)
    duration = (t_last - t_first) if (t_first and t_last) else None

    default_model = config.get("model_name", "?").split("/")[-1]
    trader_models = run_data.get("trader_models", {})
    trader_parts = []
    for tname in trade_logs:
        m = trader_models.get(tname, "").split("/")[-1] if trader_models.get(tname) else default_model
        trader_parts.append(f"{tname} ({m})")
    trader_list = ", ".join(trader_parts) or "none"
    noise_count = config.get("noise_count", len(noise_rows))
    compression = config.get("compression_ratio", "?")
    period_str = f"Period: {t_first:%b %d %H:%M} – {t_last:%H:%M} ({_format_duration(duration)})" if duration else f"Batches {first_h}–{last_h}"
    block_interval = config.get("block_interval_s", 2.0)
    batch_min = int(block_interval * compression / 60)
    llm_count = len(trade_logs)
    st.caption(
        f"{period_str} · "
        f"Batch every {batch_min} min · "
        f"{llm_count} LLM traders · "
        f"{noise_count} noise bots"
    )

    # General
    st.markdown("#### General")

    # Compute per-source volume from fills
    mm_vol = 0.0
    llm_vol = 0.0
    noise_vol = 0.0
    for b_v in blocks:
        for f in b_v.get("mm_fills", []):
            mm_vol += f["fill_qty"] * f["fill_price"]
        for f in b_v.get("trader_fills", []):
            llm_vol += f["fill_qty"] * f["fill_price"]
        for f in b_v.get("noise_fills", []):
            noise_vol += f["fill_qty"] * f["fill_price"]
    # Server volume is single-counted (each trade counted once)
    total_vol = sum(b.get("total_volume_nanos", b.get("volume_nanos", 0)) for b in blocks) / 1e9

    total_welfare = sum(b.get("welfare_nanos", 0) for b in blocks)

    # Row 0: Initial balances
    mm_init = config.get("mm_balance", 0)
    trader_init = config.get("trader_balance", 0)
    noise_init = config.get("noise_balance", 0)
    r0c1, r0c2, r0c3, r0c4 = st.columns(4)
    r0c1.metric("MM Initial", f"${mm_init:,.0f}")
    r0c2.metric("Trader Initial", f"${trader_init:,.0f}")
    r0c3.metric("Noise Initial", f"${noise_init:,.0f}")
    r0c4.metric("Noise Bots", noise_count)

    # Row 1: Batches, Total Fills, Duration (4 cols to align with rows below)
    r1c1, r1c2, r1c3, r1c4 = st.columns(4)
    r1c1.metric("Batches", len(blocks))
    r1c2.metric("Total Fills", f"{total_fills:,}")
    r1c3.metric("Duration", _format_duration(duration) if duration else "N/A")

    # Row 2: Volume breakdown
    r2c1, r2c2, r2c3, r2c4 = st.columns(4)
    r2c1.metric("Total Volume", f"${total_vol:,.0f}")
    r2c2.metric("MM Volume", f"${mm_vol:,.0f}")
    r2c3.metric("LLM Volume", f"${llm_vol:,.0f}")
    r2c4.metric("Noise Volume", f"${noise_vol:,.0f}")

    # Row 3: Welfare breakdown
    r3c1, r3c2, r3c3, r3c4 = st.columns(4)
    r3c1.metric("Total Welfare", f"${total_welfare / 1e9:,.0f}" if total_welfare else f"${mm_welfare + trader_welfare + noise_welfare:,.2f}")
    r3c2.metric("MM Welfare", f"${mm_welfare:,.2f}")
    r3c3.metric("LLM Welfare", f"${trader_welfare:,.2f}")
    r3c4.metric("Noise Welfare", f"${noise_welfare:,.2f}")

    # Market Maker & Noise
    st.markdown("#### Market Maker & Noise")
    c1, c2, c3, c4 = st.columns(4)
    c1.metric("MM PnL", f"${mm_row['pnl']:+.2f}" if mm_row else "N/A")
    if noise_pnls:
        c2.metric("Noise PnL (min)", f"${noise_pnls[0]:+.2f}")
        c3.metric("Noise PnL (max)", f"${noise_pnls[-1]:+.2f}")
        c4.metric("Noise PnL (median)", f"${statistics.median(noise_pnls):+.2f}")

    # LLM Traders
    st.markdown("#### LLM Traders")
    trades_with_orders = [t for t in all_trade_entries if t["orders"]]
    has_fill_data = any(b_f.get("trader_fills") for b_f in blocks)
    filled_vol_nanos = 0
    if has_fill_data:
        for b_fill in blocks:
            for f in b_fill.get("trader_fills", []):
                filled_vol_nanos += f["fill_qty"] * int(f["fill_price"] * 1e9)

    c1, c2, c3 = st.columns(3)
    c1.metric("Articles Processed", f"{len(all_trade_entries)} ({len(trade_logs)} traders)")
    c2.metric("Trades Executed", len(trades_with_orders))
    c3.metric("Volume Filled", f"${filled_vol_nanos / 1e9:,.0f}" if has_fill_data else "N/A (rerun needed)")

    if trader_rows:
        # Per-trader trades (articles that produced orders)
        trades_per_trader = {}
        for tname, entries in trade_logs.items():
            trades_per_trader[tname] = len([e for e in entries if e.get("orders")])

        # Per-trader filled volume from block trader_fills
        vol_per_trader: dict[str, float] = {}
        if has_fill_data:
            for b_fill in blocks:
                for f in b_fill.get("trader_fills", []):
                    src = f.get("source", "")
                    vol_per_trader[src] = vol_per_trader.get(src, 0) + f["fill_qty"] * f["fill_price"]

        rows = []
        for r in trader_rows:
            name = r["name"]
            row = {
                "Trader": name,
                "Trades": trades_per_trader.get(name, 0),
                "Volume ($)": f"${vol_per_trader.get(name, 0):,.0f}" if has_fill_data else "N/A",
                "PnL ($)": f"${r['pnl']:+.2f}",
            }
            rows.append(row)
        pnl_df = pd.DataFrame(rows)
        pnl_df = pnl_df.sort_values("PnL ($)", ascending=False).reset_index(drop=True)
        st.dataframe(pnl_df, use_container_width=True, hide_index=True)

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
            "volume": b.get("total_volume_nanos", 0) / 1e9,
            "fills": b.get("orders_filled", 0),
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

            # Collect fills from order submission until next order by same trader
            fill_str = ""
            if tl["orders"]:
                ttl_end = order_block + 4  # inclusive (TTL=5)
                remaining_tlog = [t2 for t2 in trade_logs.get(tname, [])
                                  if t2.get("orders") and t2.get("block_height", -1) > bh]
                if remaining_tlog:
                    ttl_end = min(ttl_end, remaining_tlog[0]["block_height"])
                # Cap fills to total ordered quantity
                total_ordered = 0
                for o_str in tl["orders"]:
                    parts = o_str.split()
                    if len(parts) >= 2:
                        try:
                            total_ordered += int(parts[1])
                        except ValueError:
                            pass
                remaining_cap = total_ordered
                nearby_fills = []
                for bk in blocks:
                    if remaining_cap <= 0:
                        break
                    if order_block <= bk["height"] <= ttl_end:
                        for f in bk.get("trader_fills", []):
                            if remaining_cap <= 0:
                                break
                            fsrc = f.get("source", "Trader")
                            if fsrc in (tname, "Trader"):
                                capped_qty = min(f["fill_qty"], remaining_cap)
                                nearby_fills.append({**f, "fill_qty": capped_qty})
                                remaining_cap -= capped_qty
                if nearby_fills:
                    total_filled = sum(f["fill_qty"] for f in nearby_fills)
                    avg_price = sum(f["fill_price"] * f["fill_qty"] for f in nearby_fills) / total_filled if total_filled else 0
                    fill_str = f"{total_filled} filled @ {avg_price:.4f}"
                else:
                    fill_str = "no fills"

            # Use LLM block for chart position (that's when the decision was made)
            llm_block_data = next((b for b in blocks if b["height"] == bh), None)
            yes_price = llm_block_data["yes_price"] if llm_block_data and llm_block_data["yes_price"] is not None else 0

            # Extract limit order price (YES-equivalent) from first order
            trade_price = None
            if tl["orders"]:
                for o_str in tl["orders"]:
                    parts = o_str.split()
                    if len(parts) >= 4 and parts[2] == "@":
                        try:
                            raw_price = float(parts[3].lstrip("$"))
                            if parts[0] in ("BuyNo", "SellNo"):
                                trade_price = 1.0 - raw_price
                            else:
                                trade_price = raw_price
                            break
                        except (ValueError, IndexError):
                            pass

            # Fair value (backward-compat: fall back to probability)
            fair_value = tl.get("fair_value", tl.get("probability", 0))

            trader_events.append({
                "block": bh,
                "trader": tname,
                "time": time_str,
                "yes_price": yes_price,
                "fair_value": fair_value,
                "trade_price": trade_price,
                "holdings": f"${tl.get('balance', 0):.1f} / {tl.get('yes_pos', 0)}Y / {tl.get('no_pos', 0)}N",
                "orders": orders_str,
                "fills": fill_str,
            })
    trader_df = pd.DataFrame(trader_events) if trader_events else None
    if trader_df is not None and not trader_df.empty:
        trader_df["filled"] = trader_df["fills"].str.contains(r"^\d+\s+filled", na=False, regex=True)

    try:
        import altair as alt

        only_filled = st.checkbox("Show only filled orders", value=False)
        if only_filled and trader_df is not None and not trader_df.empty:
            trader_df = trader_df[trader_df["filled"]].copy()

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

        # Compute shared x-domain so price and volume charts align
        all_blocks = list(price_df["block"])
        if trader_df is not None and not trader_df.empty:
            all_blocks.extend(trader_df["block"].tolist())
        x_domain = [min(all_blocks), max(all_blocks)]

        shared_x_axis = alt.Axis(
            values=tick_values,
            labelExpr=label_expr,
            labelAngle=315,
            labelOverlap=False,
            labelPadding=5,
        )

        base = alt.Chart(price_df).mark_line(color="#4A90D9", strokeWidth=2).encode(
            x=alt.X("block:Q", title="", axis=shared_x_axis,
                    scale=alt.Scale(domain=x_domain)),
            y=alt.Y("yes_price:Q", title="YES Price", scale=alt.Scale(zero=False)),
            tooltip=["block:Q", "time:N", alt.Tooltip("yes_price:Q", format=".4f")],
        )

        layers = [base]

        if trader_df is not None and not trader_df.empty:
            has_multi = trader_df["trader"].nunique() > 1 if "trader" in trader_df.columns else False
            color_enc = alt.Color("trader:N", title="Trader") if has_multi else alt.value("#E74C3C")

            # Trade decision dots (positioned at limit order price, YES-equivalent)
            traded_df = trader_df[trader_df["trade_price"].notna()].copy()
            no_trade_df = trader_df[trader_df["trade_price"].isna()].copy()

            common_tooltip = [
                alt.Tooltip("block:Q", title="Batch"),
                alt.Tooltip("trader:N", title="Trader"),
                alt.Tooltip("time:N", title="Sim Time"),
                alt.Tooltip("fair_value:Q", title="Fair Value", format=".2f"),
                alt.Tooltip("holdings:N", title="Holdings"),
                alt.Tooltip("orders:N", title="Orders"),
                alt.Tooltip("fills:N", title="Fills"),
            ]

            if not traded_df.empty:
                trade_dots = alt.Chart(traded_df).mark_point(
                    size=120, filled=True,
                ).encode(
                    x="block:Q",
                    y=alt.Y("trade_price:Q"),
                    color=color_enc,
                    tooltip=common_tooltip,
                )
                layers.append(trade_dots)

            # NO TRADE events: show as hollow diamonds at LLM probability
            if not no_trade_df.empty:
                no_trade_dots = alt.Chart(no_trade_df).mark_point(
                    size=80, filled=False, shape="diamond",
                ).encode(
                    x="block:Q",
                    y=alt.Y("fair_value:Q"),
                    color=color_enc,
                    tooltip=common_tooltip,
                )
                layers.append(no_trade_dots)

        price_chart = alt.layer(*layers).properties(height=400, width="container")
        st.altair_chart(price_chart, use_container_width=True)

        # Volume chart — add an invisible color legend to reserve the same space as price chart
        vol_base = alt.Chart(price_df).mark_bar(color="#7FB3D8", opacity=0.7).encode(
            x=alt.X("block:Q", title="", axis=shared_x_axis,
                    scale=alt.Scale(domain=x_domain)),
            y=alt.Y("volume:Q", title="Volume ($)"),
            tooltip=[
                alt.Tooltip("block:Q", title="Batch"),
                alt.Tooltip("time:N", title="Time"),
                alt.Tooltip("volume:Q", title="Volume ($)", format=",.2f"),
                alt.Tooltip("fills:Q", title="Fills"),
            ],
        )
        # Invisible points layer with same color encoding to reserve matching legend space
        if trader_df is not None and not trader_df.empty:
            has_multi = trader_df["trader"].nunique() > 1 if "trader" in trader_df.columns else False
            if has_multi:
                # One row per unique trader so legend reserves full width
                dummy_df = trader_df.drop_duplicates("trader")[["trader", "block"]].copy()
                dummy_legend = alt.Chart(dummy_df).mark_point(opacity=0).encode(
                    x=alt.X("block:Q"),
                    y=alt.value(0),
                    color=alt.Color("trader:N", title="Trader", legend=alt.Legend(symbolOpacity=0, labelOpacity=0, titleOpacity=0)),
                )
                vol_chart = alt.layer(vol_base, dummy_legend).properties(height=120, width="container")
            else:
                vol_chart = vol_base.properties(height=120, width="container")
        else:
            vol_chart = vol_base.properties(height=120, width="container")
        st.altair_chart(vol_chart, use_container_width=True)
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
    trader_model = trader_models.get(selected_trader, "").split("/")[-1] if trader_models.get(selected_trader) else default_model
    st.caption(f"Model: {trader_model}")
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

    # Show first N rows by default, expandable
    INITIAL_SHOW = 15
    show_all = len(trade_log) <= INITIAL_SHOW or st.checkbox(
        f"Show all {len(trade_log)} decisions (showing first {INITIAL_SHOW})", value=False, key="show_all_decisions",
    )
    visible_log = trade_log if show_all else trade_log[:INITIAL_SHOW]

    for i, t in enumerate(trade_log, 1):
        bh = t.get("block_height", -1)
        order_block = bh
        orders_str = (", ".join(t["orders"]) or "NO TRADE").replace("$", "\\$")
        mkt_price = price_at_block.get(bh)
        mkt_str = f"mkt={mkt_price:.2f}" if mkt_price is not None else "mkt=?"

        # Collect per-fill data: from order submission until next decision with orders
        # (the next order supersedes this one, so fills after that belong to it)
        per_fill_items = []  # list of (fill_block, fill_qty, fill_price)
        if t["orders"]:
            ttl_end = order_block + 4  # inclusive (TTL=5)
            # Shrink window if a later decision also placed orders
            for future_t in trade_log[i:]:  # i is 1-based, so trade_log[i:] = remaining
                if future_t.get("orders") and future_t.get("block_height", -1) > order_block:
                    ttl_end = min(ttl_end, future_t["block_height"])
                    break
            # Cap fills to the total quantity this trade entry ordered
            total_ordered = 0
            for o_str in t["orders"]:
                parts = o_str.split()
                if len(parts) >= 2:
                    try:
                        total_ordered += int(parts[1])
                    except ValueError:
                        pass
            remaining_cap = total_ordered
            for bk in blocks:
                if remaining_cap <= 0:
                    break
                if order_block < bk["height"] <= ttl_end:
                    for f in bk.get("trader_fills", []):
                        if remaining_cap <= 0:
                            break
                        # Filter to selected trader (new runs tag fills with source=trader name)
                        fsrc = f.get("source", "Trader")
                        if fsrc not in (selected_trader, "Trader"):
                            continue
                        capped_qty = min(f["fill_qty"], remaining_cap)
                        per_fill_items.append((bk["height"], capped_qty, f["fill_price"]))
                        remaining_cap -= capped_qty

        # Summary for header
        if per_fill_items:
            total_filled = sum(qty for _, qty, _ in per_fill_items)
            avg_price = sum(p * q for _, q, p in per_fill_items) / total_filled if total_filled else 0
            fill_str = f" · {total_filled} filled @ {avg_price:.4f}"
        elif t["orders"]:
            fill_str = " · no fills"
        else:
            fill_str = ""

        fv = t.get("fair_value", t.get("probability", 0))
        header = (
            f"[{i}] Batch {order_block} · "
            f"{mkt_str} → FV={fv:.2f} · "
            f"{orders_str}{fill_str}"
        )

        # Only render expanders for visible rows
        if i <= len(visible_log):
            with st.expander(header):
                # Timing info
                if order_block != bh:
                    st.markdown(f"**LLM call initiated:** batch {bh}")

                # Holdings at decision time
                bal = t.get("balance", 0)
                yp = t.get("yes_pos", 0)
                np_ = t.get("no_pos", 0)
                if bal or yp or np_:
                    total_val = bal + yp * (mkt_price or 0) + np_ * (1 - (mkt_price or 0))
                    holdings = f"**Holdings:** \\${bal:.2f} cash · YES {yp} · NO {np_} · total ~\\${total_val:.2f}"
                    st.markdown(holdings)

                st.markdown(f"**Orders:** {orders_str}, batch {order_block}" if t["orders"] else "**No trade**")

                # Per-fill breakdown
                if per_fill_items:
                    lines = ["**Fills:**"]
                    for fill_block, fill_qty, fill_price in per_fill_items:
                        lines.append(f"- {fill_qty} filled @ {fill_price:.4f}, batch {fill_block}")
                    st.markdown("\n".join(lines))
                elif t["orders"]:
                    st.markdown("**Fills:** no fills")

                # Article info
                st.markdown("---")
                is_rebalance = (
                    not t.get("articles")
                    and t.get("motivation", "").startswith("[REBALANCE]")
                )
                if is_rebalance:
                    st.markdown("**Portfolio Review** (periodic rebalance)")
                else:
                    articles_list = t.get("articles", [])
                    if articles_list and len(articles_list) > 1:
                        st.markdown(f"**Articles ({len(articles_list)}):**")
                        for ai, art in enumerate(articles_list, 1):
                            st.markdown(f"{ai}. **{art['source']}** — {art['title']}")
                    else:
                        st.markdown(f"**Article:** {t.get('article_source', '')} — {t.get('article_title', '')}")

                # Analysis (new format) or motivation
                analysis = t.get("analysis")
                if analysis:
                    st.markdown(f"**LLM Analysis:**")
                    st.info(analysis.replace("$", "\\$"))
                if t.get("motivation"):
                    st.markdown(f"**Motivation:** {t['motivation'].replace('$', chr(92) + '$')}")

                # Full LLM response
                raw = t.get("raw_llm_response") or t.get("llm_response")
                if raw:
                    st.markdown("**Full LLM Response:**")
                    st.text(raw)

    # ═══════════ PER-BLOCK TABLE ═══════════
    st.subheader("Per-Batch Activity")
    block_table = []
    for b in blocks:
        t = block_times.get(b["height"])
        # active_trader_orders includes TTL carry-over (newer runs);
        # fall back to len(trader_orders) for older run data.
        active_count = b.get("active_trader_orders", len(b["trader_orders"]))
        row = {
            "Batch": b["height"],
            "Time": t.strftime("%H:%M:%S") if t else "",
            "Clearing Price": f"{b['yes_price']:.4f}" if b["yes_price"] is not None else "",
            "Volume ($)": f"{b['volume_nanos'] / 2e9:.2f}",
            "Welfare ($)": f"{b['welfare_nanos'] / 1e9:.2f}" if b.get("welfare_nanos") else "",
            "Fills": b.get("orders_filled", ""),
            "Orders (MM/LLM/Noise)": f"{len(b['mm_orders'])}/{active_count}/{b['noise_order_count']}",
            "LLM Orders": "",
        }
        # trader_llm: list (new) or dict/None (old runs)
        raw_llm = b["trader_llm"]
        llm_list = raw_llm if isinstance(raw_llm, list) else ([raw_llm] if raw_llm else [])
        # Only show LLM entries for traders that actually placed orders
        # Orders appear in the NEXT block (h+1) due to submission timing
        traders_with_orders = set()
        next_block = next((nb for nb in blocks if nb["height"] == b["height"] + 1), None)
        for src in [b, next_block] if next_block else [b]:
            for entry in src.get("trader_orders_detail", []):
                tname = entry.get("trader")
                if tname:
                    traders_with_orders.add(tname)
        if llm_list:
            parts = []
            for l in llm_list:
                trader_name = l.get("trader", "")
                if trader_name and trader_name not in traders_with_orders:
                    continue
                tag = f"[{trader_name}] " if trader_name else ""
                fv = l.get("fair_value", l.get("probability", 0))
                parts.append(f"{tag}FV={fv:.2f}")
            row["LLM Orders"] = " | ".join(parts)
        block_table.append(row)

    block_df = pd.DataFrame(block_table)
    st.dataframe(
        block_df.style.map(
            lambda v: "color: #f0c050" if v else "",
            subset=["LLM Orders"],
        ),
        hide_index=True,
        use_container_width=True,
        height=400,
    )

    # ═══════════ BLOCK INSPECTOR ═══════════
    st.subheader("Batch Inspector")

    block_heights = [b["height"] for b in blocks]
    selected_block = st.selectbox("Select batch", block_heights)
    b = next(b for b in blocks if b["height"] == selected_block)
    active_orders = live_orders_by_block.get(b["height"], [])
    submitted_orders = all_orders_by_block.get(b["height"], [])

    # ── Orderbook depth chart ──
    st.markdown("**Orderbook (YES-equivalent)**")
    st.caption(
        "BuyYes and SellNo are both bids for YES (bullish). "
        "BuyNo and SellYes are both asks on YES (bearish). "
        "Same directional bet, different mechanics: Buy uses cash, Sell uses inventory. "
        "Includes carried-over orders from previous batches (TTL=5)."
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
            use_log = st.checkbox("Log scale (Y axis)", value=False, key="ob_log_scale")
            if use_log:
                book_df = book_df[book_df["quantity"] > 0].copy()
            y_scale = alt.Scale(type="symlog", constant=1) if use_log else alt.Scale()
            ob_chart = alt.Chart(book_df).mark_bar().encode(
                x=alt.X("price:Q", title="YES Price", scale=alt.Scale(zero=False)),
                y=alt.Y("quantity:Q", title="Quantity", stack=None if use_log else True,
                        scale=y_scale),
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
        st.caption("No active orders in this batch")

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
        orders_with_fills = sum(1 for o in submitted_orders if o.get("filled_this_block", 0) > 0)
        fill_rate = (orders_with_fills / orders_submitted * 100) if orders_submitted > 0 else 0

        # Per-source welfare for this batch
        batch_welfare: dict[str, float] = {}
        if yes_price is not None:
            for o in submitted_orders:
                filled = o.get("filled_this_block", 0)
                if filled <= 0:
                    continue
                if o["side"] == "BuyYes":
                    w = (o["price"] - yes_price) * filled
                elif o["side"] == "BuyNo":
                    w = (o["price"] - (1.0 - yes_price)) * filled
                elif o["side"] == "SellYes":
                    w = (yes_price - o["price"]) * filled
                else:  # SellNo
                    w = ((1.0 - yes_price) - o["price"]) * filled
                batch_welfare[o["source"]] = batch_welfare.get(o["source"], 0) + w
        batch_mm_w = batch_welfare.get("MM", 0)
        batch_noise_w = batch_welfare.get("Noise", 0)
        batch_trader_w = sum(v for k, v in batch_welfare.items() if k not in ("MM", "Noise"))

        ec1, ec2, ec3 = st.columns(3)
        ec1.metric("Welfare", f"${welfare_nanos / 1e9:,.2f}")
        ec2.metric("Volume", f"${total_vol_nanos / 1e9:,.2f}")
        ec3.metric("Fill Rate", f"{orders_with_fills}/{orders_submitted} ({fill_rate:.0f}%)")

        # Per-source volume for this batch
        batch_mm_vol = sum(f["fill_qty"] * f["fill_price"] for f in b.get("mm_fills", []))
        batch_llm_vol = sum(f["fill_qty"] * f["fill_price"] for f in b.get("trader_fills", []))
        batch_noise_vol = sum(f["fill_qty"] * f["fill_price"] for f in b.get("noise_fills", []))
        vc1, vc2, vc3 = st.columns(3)
        vc1.metric("MM Volume", f"${batch_mm_vol:,.2f}")
        vc2.metric("LLM Volume", f"${batch_llm_vol:,.2f}")
        vc3.metric("Noise Volume", f"${batch_noise_vol:,.2f}")

        wc1, wc2, wc3 = st.columns(3)
        wc1.metric("MM Welfare", f"${batch_mm_w:,.2f}")
        wc2.metric("LLM Welfare", f"${batch_trader_w:,.2f}")
        wc3.metric("Noise Welfare", f"${batch_noise_w:,.2f}")

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
    # Use the pre-computed order snapshots which already have filled_this_block
    for source in sorted_sources:
        orders = [o for o in submitted_orders if o["source"] == source]
        fills = fills_by_source.get(source, [])
        if not orders and not fills:
            continue

        # Build fill lookup: side → list of fill records (for price/delta info)
        side_fills: dict[str, list[dict]] = {}
        for f in fills:
            side_fills.setdefault(_fill_side(f), []).append(f)

        # Build rows from orders (which already know their filled_this_block)
        rows = []
        for o in orders:
            filled_now = o.get("filled_this_block", 0)
            is_carried = o["submitted_block"] < b["height"]
            age = b["height"] - o["submitted_block"]

            # Origin label
            if is_carried:
                origin = f"blk-{age}"
            else:
                origin = "new"

            row = {
                "Side": o["side"],
                "Qty": o["original_qty"],
                "Limit": f"${o['price']:.2f}",
                "Origin": origin,
            }

            if filled_now > 0:
                # Pop a matching fill record for price/delta info
                matched = side_fills.get(o["side"], [])
                f = matched.pop(0) if matched else None
                if f:
                    deltas = ", ".join(
                        f"{d['outcome']} {d['delta']:+d}" for d in f.get("position_deltas", [])
                    )
                    # Welfare: buy = (limit - clearing), sell = (clearing - limit)
                    if o["side"] == "BuyYes":
                        w = (o["price"] - (yes_price or f["fill_price"])) * filled_now
                    elif o["side"] == "BuyNo":
                        w = (o["price"] - (1.0 - yes_price if yes_price is not None else f["fill_price"])) * filled_now
                    elif o["side"] == "SellYes":
                        w = ((yes_price or f["fill_price"]) - o["price"]) * filled_now
                    else:  # SellNo
                        w = ((1.0 - yes_price if yes_price is not None else f["fill_price"]) - o["price"]) * filled_now
                    row["Filled"] = filled_now
                    row["Fill $"] = f"${f['fill_price']:.4f}"
                    row["Volume"] = f"${filled_now * f['fill_price']:.2f}"
                    row["Welfare"] = f"${w:.2f}"
                    row["Δ Pos"] = deltas
                else:
                    row["Filled"] = filled_now
                    row["Fill $"] = ""
                    row["Volume"] = ""
                    row["Welfare"] = ""
                    row["Δ Pos"] = ""
            else:
                row["Filled"] = ""
                row["Fill $"] = ""
                row["Volume"] = ""
                row["Welfare"] = ""
                row["Δ Pos"] = ""
            rows.append(row)

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
        n_new = sum(1 for o in orders if o["submitted_block"] == b["height"])
        n_carried = len(orders) - n_new
        n_filled_this_block = sum(1 for o in orders if o.get("filled_this_block", 0) > 0)
        carried_label = f" (+{n_carried} carried)" if n_carried else ""
        fill_label = f", {n_filled_this_block} filled" if n_filled_this_block else ""
        fills_label = f", {len(fills)} fills (prior orders)" if fills and not n_filled_this_block else ""
        with st.expander(f"**{source}**: {n_new} orders{carried_label}{fill_label}{fills_label}", expanded=is_trader):
            if rows:
                st.dataframe(rows, use_container_width=True, hide_index=True)
            else:
                st.caption("No activity")



def main():
    st.set_page_config(page_title=f"{_market_name.title()} Simulation", layout="wide")

    # Shrink metric values so section headers are visually dominant
    st.markdown("""<style>
    [data-testid="stMetricValue"] { font-size: 1.1rem; }
    [data-testid="stMetricLabel"] { font-size: 0.85rem; }
    </style>""", unsafe_allow_html=True)

    st.title(f"{_market_config.question} — Simulation")

    df = load_data()

    # Filter to January 2026
    from datetime import date as _date
    filtered = df[(df["date"] >= _date(2026, 1, 1)) & (df["date"] <= _date(2026, 2, 5))].copy()

    # Filter out disabled bots (e.g. israeli_trader)
    active_bots = {k: v for k, v in BOT_PERSONAS.items()
                   if v.get("enabled", True) and v.get("sources")}

    # ── Tabs ──
    all_tabs = st.tabs(["Simulation", "LLM Traders", "News Sources", "Daily Explorer"])
    tab_simulation = all_tabs[0]
    tab_traders = all_tabs[1]
    tab_summary = all_tabs[2]
    tab_daily = all_tabs[3]

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

        with st.expander("How were these articles collected?"):
            st.markdown(
                "**Source:** [GDELT Project](https://www.gdeltproject.org/) — the world's "
                "largest open news monitoring platform, tracking news from virtually every "
                "country in over 100 languages.\n\n"
                "**Method:** Articles were fetched via the GDELT DOC 2.0 API in 2-hour sliding windows. "
                "Two complementary queries were run and merged:\n\n"
                "1. **Broad query:** (iran OR tehran OR iranian) AND "
                "(strike OR attack OR military OR war OR nuclear OR sanctions OR missile)\n"
                '2. **Diplomacy query:** (iran OR tehran OR iranian) AND '
                '(trump OR pentagon OR "united states") AND '
                "(negotiations OR deal OR diplomacy OR talks OR agreement OR ceasefire OR peace OR treaty)\n\n"
                "**Processing pipeline:**\n\n"
                "1. **Fetch** — Raw articles collected from GDELT with metadata (title, source, country, language, timestamp)\n"
                "2. **Merge & deduplicate** — Both query results combined, deduplicated by URL"
            )


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
        with st.expander("How the simulation works"):
            st.markdown(
                "**Market mechanism: Frequent Batch Auctions (FBA)**\n\n"
                "Unlike traditional order books where orders execute one-by-one, "
                "all orders in a batch are collected and cleared simultaneously at a single price. "
                "This eliminates front-running and ensures fair price discovery — "
                "every participant in a batch gets the same clearing price regardless of submission order.\n\n"
                "**Matching engine**\n\n"
                "The Rust-based matching engine solves a welfare-maximizing optimization problem each batch: "
                "it finds clearing prices and fill quantities that maximize total trader surplus. "
                "BuyYes + BuyNo orders can match via minting (total cost = \\$1). "
                "All arithmetic is in integer nanos (1 dollar = 1,000,000,000 nanos) — no floating point.\n\n"
                "**Time compression**\n\n"
                "The simulation compresses real calendar time — each simulated day runs in minutes of wall-clock time. "
                "Block production interval and compression ratio are configurable. "
                "Articles arrive at their original publication timestamps within the compressed timeline.\n\n"
                "**Participants**\n\n"
                "- **Market Maker (MM)** — a two-sided liquidity provider that continuously quotes "
                "buy prices on both YES and NO outcomes. Quotes at multiple price levels with tapered sizing "
                "(larger near the mid, smaller at outer levels). Spread widens automatically when volatility "
                "spikes and narrows in calm markets. When price momentum is detected, quoting shifts "
                "asymmetrically to avoid being picked off by informed flow. Inventory management gradually "
                "increases sell pressure as positions grow, and unwinds matched pairs to free capital.\n"
                "- **Noise traders** — 20 bots placing random orders each block with 50% probability, "
                "providing baseline order flow.\n"
                "- **LLM traders** — autonomous agents powered by language models. "
                "Each receives news articles matching their persona and makes independent trading decisions. "
                "See the LLM Traders tab for details.\n\n"
                "**Order mechanics**\n\n"
                "- Limit price = worst price you'd accept. FBA guarantees you get the clearing price (which is better)\n"
                "- Orders persist for 3 batches (TTL=3) if not filled\n\n"
                "**Multi-day simulation**\n\n"
                "Positions and balances carry over between simulated days. "
                "Each day, fresh articles are loaded and LLM traders continue from their prior state — "
                "portfolio, trade history, and last reasoning are preserved for continuity.\n\n"
                "**What you'll find below**\n\n"
                "- **Summary** — key metrics: total volume, fills, welfare breakdown by participant type\n"
                "- **Price + Trader Activity** — interactive chart showing YES price over time, "
                "overlaid with LLM trader fair value estimates and trade markers\n"
                "- **Leaderboard** — final P&L ranking across all participants\n"
                "- **LLM Decisions** — chronological log of every LLM trader decision: "
                "article received, analysis, fair value, orders placed, and fill results\n"
                "- **Per-Batch Activity** — volume and fill count per block over time\n"
                "- **Batch Inspector** — drill into any individual batch to see all orders, "
                "fills, and the clearing price"
            )
        render_simulation_tab()

    # ═══════════════════════════ LLM TRADERS ═══════════════════════════
    with tab_traders:
        with st.expander("How LLM traders work"):
            st.markdown(
                "Each LLM trader is an autonomous agent powered by a language model "
                "(Gemini 3.1 Flash Lite via OpenRouter). Traders receive news articles "
                "in real-time during the simulation and make independent trading decisions.\n\n"
                "**Per-trader configuration:**\n"
                "- **News sources** — each trader reads from a curated set of outlets "
                "matching their geographic/thematic focus (e.g. Arab press, US media, financial outlets)\n"
                "- **Headline filter** — before the simulation, an LLM pre-screens every headline "
                "from the trader's sources, keeping only articles relevant to the market question. "
                "Full article text is then fetched for accepted headlines\n"
                "- **Persona** — defines how the trader interprets signals and trades: "
                "identity, reading style, and trading style\n\n"
                "**During simulation, each block the trader receives:**\n"
                "- New articles that have arrived since the last block\n"
                "- Current market price and recent price trend\n"
                "- Their portfolio state (cash, positions, P&L)\n"
                "- History of their recent trades and fill results\n"
                "- Their own prior reasoning (for self-reflection)\n\n"
                "**The trader responds with:**\n"
                "- ANALYSIS — interpretation of the new information\n"
                "- FAIR_VALUE — their probability estimate for the event\n"
                "- EDGE — difference vs market price; only trades if edge > \\$0.03\n"
                "- ORDERS — buy/sell/hold with limit prices\n\n"
                "**Portfolio rebalancing** runs every 4 simulated hours. "
                "Traders with open positions are prompted to review and "
                "optionally trim or exit positions to lock in profits or cut losses."
            )

        trader_names = {k: v["name"] for k, v in active_bots.items()}
        selected_trader = st.selectbox(
            "Select trader",
            options=list(trader_names.keys()),
            format_func=lambda k: trader_names[k],
        )
        render_bot_tab(filtered, selected_trader, active_bots[selected_trader])


if __name__ == "__main__":
    main()
