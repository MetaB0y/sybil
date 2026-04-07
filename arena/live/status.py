"""CLI dashboard — plain-text summary of arena state for non-browser access.

Usage:
    python -m live.status                    # local
    docker exec sybil-arena python -m live.status  # on server
"""

import json
import sqlite3
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path


def extract_strategy(name: str) -> str:
    if "(Kelly)" in name:
        return "Kelly"
    elif "(Flat)" in name:
        return "Flat"
    elif name.startswith("Noise"):
        return "Noise"
    return "Legacy"


def run(db_path: str | None = None, hours: int = 24):
    if db_path is None:
        db_path = "/data/decisions.db" if Path("/data").exists() else str(Path(__file__).parent / "decisions.db")

    if not Path(db_path).exists():
        print(f"DB not found: {db_path}")
        return

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    cutoff = (datetime.now(timezone.utc) - timedelta(hours=hours)).isoformat()

    print(f"=== Sybil Arena Status ({datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M')} UTC, last {hours}h) ===\n")

    # --- Strategy Comparison ---
    rows = conn.execute(
        "SELECT trader_name, balance, portfolio_value, pnl, positions "
        "FROM portfolio_snapshots WHERE id IN ("
        "  SELECT MAX(id) FROM portfolio_snapshots GROUP BY trader_name"
        ") ORDER BY trader_name"
    ).fetchall()

    if not rows:
        print("No portfolio snapshots yet.\n")
    else:
        print("--- Strategy Comparison ---")
        strategies = {}
        for r in rows:
            s = extract_strategy(r["trader_name"])
            strategies.setdefault(s, []).append(r)

        for strat in ["Kelly", "Flat", "Legacy", "Noise"]:
            traders = strategies.get(strat, [])
            if not traders:
                continue
            total_pnl = sum(t["pnl"] for t in traders)
            avg_pnl = total_pnl / len(traders) if traders else 0
            total_positions = 0
            for t in traders:
                positions = json.loads(t["positions"]) if t["positions"] else {}
                total_positions += sum(
                    1 for mp in positions.values() for q in mp.values() if q != 0
                )
            print(f"  {strat:8s}  traders={len(traders)}  PnL=${total_pnl:+8.2f}  avg=${avg_pnl:+7.2f}  positions={total_positions}")

        kelly_pnl = sum(t["pnl"] for t in strategies.get("Kelly", []))
        flat_pnl = sum(t["pnl"] for t in strategies.get("Flat", []))
        leader = "Kelly" if kelly_pnl > flat_pnl else "Flat" if flat_pnl > kelly_pnl else "Tied"
        print(f"\n  Leader: {leader} (Kelly ${kelly_pnl:+.2f} vs Flat ${flat_pnl:+.2f}, gap ${abs(kelly_pnl - flat_pnl):.2f})")
        print()

    # --- Per-Trader Portfolio ---
    if rows:
        print("--- Portfolio Summary ---")
        for r in rows:
            s = extract_strategy(r["trader_name"])
            positions = json.loads(r["positions"]) if r["positions"] else {}
            n_pos = sum(1 for mp in positions.values() for q in mp.values() if q != 0)
            print(f"  {r['trader_name']:30s}  cash=${r['balance']:8.2f}  value=${r['portfolio_value']:8.2f}  PnL=${r['pnl']:+7.2f}  pos={n_pos}")
        print()

    # --- Fair Value Drift ---
    fv_rows = conn.execute(
        "SELECT trader_name, market_name, fair_value, market_price, timestamp "
        "FROM decisions WHERE timestamp > ? ORDER BY timestamp",
        (cutoff,)
    ).fetchall()

    if fv_rows:
        print("--- Fair Value Drift (last decisions per market) ---")
        # Group by (trader, market) and take latest
        latest = {}
        trends = {}
        for r in fv_rows:
            key = (r["trader_name"], r["market_name"])
            latest[key] = r
            trends.setdefault(key, []).append(r["fair_value"])

        warnings = []
        for (trader, market), r in sorted(latest.items()):
            fv = r["fair_value"]
            mkt = r["market_price"]
            edge = abs(fv - mkt)
            trend_vals = trends[(trader, market)][-5:]
            trend_str = " -> ".join(f"{v:.2f}" for v in trend_vals)
            # Only warn if FV is extreme AND market disagrees (divergence, not consensus)
            fv_extreme = fv > 0.85 or fv < 0.15
            mkt_agrees = (mkt > 0.80 and fv > 0.85) or (mkt < 0.20 and fv < 0.15)
            warn = " !! DIVERGENT" if fv_extreme and not mkt_agrees else ""
            if warn:
                warnings.append((trader, market, fv))
            if edge > 0.02 or warn:  # Only show if meaningful edge or warning
                print(f"  {trader:30s} | {market[:40]:40s} | FV={fv:.2f} mkt={mkt:.2f} edge={edge:.2f} | {trend_str}{warn}")

        if warnings:
            print(f"\n  !! {len(warnings)} extreme FV(s) — possible conviction loop")
        print()

    # --- Recent Decisions ---
    dec_rows = conn.execute(
        "SELECT trader_name, market_name, fair_value, market_price, orders, "
        "       motivation, timestamp "
        "FROM decisions WHERE timestamp > ? ORDER BY id DESC LIMIT 15",
        (cutoff,)
    ).fetchall()

    if dec_rows:
        print("--- Recent Decisions (last 15) ---")
        for r in dec_rows:
            orders = json.loads(r["orders"]) if r["orders"] else []
            orders_str = ", ".join(f"{o['side']} {o['qty']}@${o['price']:.2f}" for o in orders) if orders else "HOLD"
            edge = abs(r["fair_value"] - r["market_price"])
            ts = r["timestamp"][:16] if r["timestamp"] else ""
            print(f"  {ts} {r['trader_name']:30s} | {r['market_name'][:35]:35s} | FV={r['fair_value']:.2f} mkt={r['market_price']:.2f} edge={edge:.2f} | {orders_str}")
            if r["motivation"]:
                print(f"    {r['motivation'][:100]}")
        print()

    # --- LLM Cost ---
    has_token_table = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='token_usage'"
    ).fetchone()
    if has_token_table:
        cost_row = conn.execute(
            "SELECT SUM(prompt_tokens), SUM(completion_tokens), COUNT(*) FROM token_usage"
        ).fetchone()
        if cost_row[0]:
            total_tokens = cost_row[0] + cost_row[1]
            cost = total_tokens * 0.70 / 1_000_000
            print(f"--- LLM Cost ---")
            print(f"  Total calls: {cost_row[2]}  tokens: {total_tokens:,}  est. cost: ${cost:.4f}")
            print()

    # --- Stats ---
    total_decisions = conn.execute("SELECT COUNT(*) FROM decisions").fetchone()[0]
    total_articles = conn.execute("SELECT COUNT(*) FROM articles").fetchone()[0]
    total_snapshots = conn.execute("SELECT COUNT(*) FROM portfolio_snapshots").fetchone()[0]
    print(f"--- Stats ---")
    print(f"  Decisions: {total_decisions}  Articles: {total_articles}  Snapshots: {total_snapshots}")

    conn.close()


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Arena status (text)")
    parser.add_argument("--db", default=None)
    parser.add_argument("--hours", type=int, default=24)
    args = parser.parse_args()
    run(args.db, args.hours)
