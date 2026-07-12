"""CLI dashboard — plain-text rendering of shared arena queries.

Usage:
    python -m live.status                    # local
    docker exec sybil-arena python -m live.status  # on server
"""

import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

try:
    from . import queries
    from .sqlite_utils import connect_reader
except ImportError:
    import queries  # type: ignore[no-redef]
    from sqlite_utils import connect_reader  # type: ignore[no-redef]


def run(db_path: str | None = None, hours: int = 24):
    if db_path is None:
        db_path = "/data/decisions.db" if Path("/data").exists() else str(Path(__file__).parent / "decisions.db")
    if not Path(db_path).exists():
        print(f"DB not found: {db_path}")
        return

    conn = connect_reader(db_path)
    now_dt = datetime.now(timezone.utc)
    cutoff = (now_dt - timedelta(hours=hours)).isoformat()
    now = now_dt.strftime("%Y-%m-%d %H:%M")

    print(f"=== Sybil Arena Status ({now} UTC, last {hours}h) ===\n")

    experiments = queries.get_live_experiment_status(conn)
    if experiments:
        print("--- Experiment Records ---")
        for experiment in experiments:
            started = datetime.fromisoformat(experiment["started_at_utc"])
            if started.tzinfo is None:
                started = started.replace(tzinfo=timezone.utc)
            eligible = started + timedelta(hours=24)
            elapsed_hours = max(0.0, (now_dt - started).total_seconds() / 3600)
            window_age = (
                f"{elapsed_hours:.1f}/24h"
                if now_dt < eligible
                else ">=24h (continuity still requires report validation)"
            )
            config = experiment["configuration"]
            cohort = ",".join(str(mid) for mid in config.get("market_ids", [])) or "none"
            print(
                f"  {experiment['experiment_id']}  age={window_age}"
                f"  start={started.astimezone(timezone.utc).isoformat()}"
                f"  eligible={eligible.astimezone(timezone.utc).isoformat()}"
            )
            print(
                f"    mode={experiment['mode']}  model={config.get('model', 'unknown')}"
                f"  cohort={cohort}"
            )
            if experiment["identity_error"] is not None:
                print(f"    IDENTITY INVALID: {experiment['identity_error']}")
            expected = experiment["expected_traders_per_arm"]
            for variant in ("control", "stage1"):
                arm = experiment["arms"][variant]
                readiness = "observed" if arm["ready"] else "INCOMPLETE"
                last = arm["last_decision_at"] or "none"
                print(
                    f"    {variant:7s} {readiness:10s}"
                    f" decisions={arm['decision_count']}"
                    f" traders={arm['decision_traders']}/{expected}"
                    f" snapshots={arm['snapshot_count']}"
                    f" snapshot-traders={arm['snapshot_traders']}/{expected}"
                    f" last={last}"
                )
        print()

    # --- Strategy Comparison ---
    strat = queries.get_strategy_comparison(conn)
    if strat is not None:
        print("--- Strategy Comparison ---")
        for _, r in strat.iterrows():
            print(f"  {r['strategy']:8s}  traders={int(r['traders'])}  PnL=${r['total_pnl']:+8.2f}"
                  f"  avg=${r['avg_pnl']:+7.2f}  positions={int(r['positions'])}"
                  f"  edge={r.get('avg_edge', 0):.3f}")
        kelly = strat[strat["strategy"] == "Kelly"]["total_pnl"].sum()
        flat = strat[strat["strategy"] == "Flat"]["total_pnl"].sum()
        leader = "Kelly" if kelly > flat else "Flat" if flat > kelly else "Tied"
        print(f"\n  Leader: {leader} (Kelly ${kelly:+.2f} vs Flat ${flat:+.2f}, gap ${abs(kelly - flat):.2f})")
    else:
        print("No strategy data yet.")

    # Also show Legacy/Noise totals
    snaps = queries.get_latest_snapshots(conn)
    if not snaps.empty and "strategy" in snaps.columns:
        for label in ["Legacy", "Noise"]:
            group = snaps[snaps["strategy"] == label]
            if not group.empty:
                print(f"  {label:8s}  traders={len(group)}  PnL=${group['pnl'].sum():+8.2f}")
    print()

    # --- Portfolio Summary ---
    if not snaps.empty:
        print("--- Portfolio Summary ---")
        for _, r in snaps.iterrows():
            pos = json.loads(r["positions"]) if r["positions"] else {}
            n = sum(1 for mp in pos.values() for q in mp.values() if q != 0)
            print(f"  {r['trader_name']:30s}  cash=${r['balance']:8.2f}  value=${r['portfolio_value']:8.2f}"
                  f"  PnL=${r['pnl']:+7.2f}  pos={n}  orders={int(r['total_orders'])}  fills={int(r['total_fills'])}")
        print()

    # --- FV Drift ---
    fv = queries.get_fv_drift(conn, cutoff=cutoff)
    if not fv.empty:
        # Only show meaningful entries
        interesting = fv[(fv["edge"] > 0.02) | (fv["warning"] != "")]
        if not interesting.empty:
            print("--- Fair Value Drift ---")
            for _, r in interesting.sort_values(["warning", "edge"], ascending=[True, False]).iterrows():
                warn = f" !! {r['warning']}" if r["warning"] else ""
                print(f"  {r['trader_name']:30s} | {r['market_name'][:40]:40s} | "
                      f"FV={r['current_fv']:.2f} mkt={r['current_mkt']:.2f} edge={r['edge']:.2f} | "
                      f"{r['fv_trend']}{warn}")
            n_warn = (fv["warning"] != "").sum()
            if n_warn:
                print(f"\n  !! {n_warn} divergent FV(s)")
            print()

    # --- Recent Decisions ---
    dec = queries.get_recent_decisions(conn, cutoff=cutoff, limit=15)
    if not dec.empty:
        print("--- Recent Decisions (last 15) ---")
        for _, r in dec.iterrows():
            orders = json.loads(r["orders"]) if r["orders"] else []
            orders_str = ", ".join(f"{o['side']} {o['qty']}@${o['price']:.2f}" for o in orders) if orders else "HOLD"
            edge = abs(r["fair_value"] - r["market_price"])
            ts = r["timestamp"][:16] if r["timestamp"] else ""
            print(f"  {ts} {r['trader_name']:30s} | {r['market_name'][:35]:35s} | "
                  f"FV={r['fair_value']:.2f} mkt={r['market_price']:.2f} edge={edge:.2f} | {orders_str}")
            if r["motivation"]:
                print(f"    {r['motivation'][:100]}")
        print()

    # --- LLM Cost ---
    cost_df = queries.get_llm_cost(conn)
    if cost_df is not None and not cost_df.empty:
        total_tokens = cost_df["prompt_tokens"].sum() + cost_df["completion_tokens"].sum()
        cost = total_tokens * 0.70 / 1_000_000
        print("--- LLM Cost ---")
        print(f"  Total calls: {int(cost_df['calls'].sum())}  tokens: {int(total_tokens):,}  est. cost: ${cost:.4f}\n")

    # --- Market Maker ---
    mm = queries.get_mm_mtm()
    if mm:
        print("--- Market Maker (MtM) ---")
        print(f"  Cash: ${mm['cash']:,.2f}  Positions: ${mm['position_value']:,.2f}  ({mm['positions']} markets)")
        print(f"  Total: ${mm['total']:,.2f}  PnL: ${mm['pnl']:+,.2f} ({mm['return_pct']:+.4f}%)\n")

    # --- Stats ---
    stats = queries.get_stats(conn)
    print("--- Stats ---")
    print(f"  Decisions: {stats['decisions']}  Articles: {stats['articles']}  Snapshots: {stats['snapshots']}")

    conn.close()


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Arena status (text)")
    parser.add_argument("--db", default=None)
    parser.add_argument("--hours", type=int, default=24)
    args = parser.parse_args()
    run(args.db, args.hours)
