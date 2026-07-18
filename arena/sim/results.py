"""Simulation results: block record construction and saving."""

import json
import logging
import os
from dataclasses import asdict
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

from sybil_client.types import NANOS_PER_DOLLAR, PricePoint

log = logging.getLogger(__name__)


async def _fetch_all_fills(client, account_id: int) -> list:
    """Fetch all fills for an account, paginating if needed."""
    all_fills = []
    cursor = "0.0"
    while True:
        batch = await client.get_account_fills(account_id, limit=100, after=cursor)
        all_fills.extend(batch)
        if len(batch) < 100:
            break
        cursor = batch[-1].cursor
    return all_fills


def build_block_records(
    all_bots, mm, noise_bots, traders: list, price_history: list[PricePoint],
    trader_fills_map: dict[str, list] | None = None,
    mm_fills: list | None = None,
    noise_fills: list | None = None,
    sim_time_by_height: dict[int, datetime] | None = None,
    after_block: int = 0,
) -> list[dict]:
    """Join per-bot block_logs with server price history into per-block records."""
    from .llm_trader import _describe_order

    if trader_fills_map is None:
        trader_fills_map = {}

    all_heights: set[int] = set()
    for bot in all_bots:
        for height, _ in bot.block_log:
            if height > after_block:
                all_heights.add(height)

    price_by_height = {pt.height: pt for pt in price_history if pt.height > after_block}
    all_heights.update(price_by_height.keys())

    llm_by_block: dict[int, list[dict]] = {}
    for t in traders:
        for rec in t.trade_log:
            if rec.block_height > after_block:
                n = len(rec.articles)
                if n == 0:
                    title = "[REBALANCE]"
                    source = ""
                elif n == 1:
                    title = rec.articles[0].title
                    source = rec.articles[0].source
                else:
                    title = f"{n} articles: " + ", ".join(a.title[:40] for a in rec.articles)
                    sources = list(dict.fromkeys(a.source for a in rec.articles))
                    source = sources[0] if len(sources) == 1 else "multiple"
                llm_by_block.setdefault(rec.block_height, []).append({
                    "trader": t.name,
                    "article_title": title,
                    "article_source": source,
                    "article_count": n,
                    "fair_value": rec.fair_value,
                    "analysis": rec.analysis,
                    "motivation": rec.motivation,
                    "llm_response": rec.raw_llm_response,
                    "llm_duration_s": rec.llm_duration_s,
                })

    def _index_fills(raw_fills: list | None, source: str) -> dict[int, list[dict]]:
        by_height: dict[int, list[dict]] = {}
        if raw_fills:
            for f in raw_fills:
                if f.block_height <= after_block:
                    continue
                deltas = [
                    {"market_id": d.market_id, "outcome": d.outcome, "delta": d.delta}
                    for d in f.position_deltas
                ]
                by_height.setdefault(f.block_height, []).append({
                    "source": source,
                    "order_id": f.order_id,
                    "fill_qty": f.fill_qty,
                    "fill_price": f.fill_price_nanos / NANOS_PER_DOLLAR,
                    "position_deltas": deltas,
                })
        return by_height

    all_trader_fills_by_height: dict[int, list[dict]] = {}
    for tname, fills in trader_fills_map.items():
        for h, entries in _index_fills(fills, tname).items():
            all_trader_fills_by_height.setdefault(h, []).extend(entries)
    mm_fills_by_height = _index_fills(mm_fills, "MM")
    noise_fills_by_height = _index_fills(noise_fills, "Noise")

    mm_by_height: dict[int, list] = {}
    for h, orders in mm.block_log:
        if h <= after_block:
            continue
        mm_by_height.setdefault(h + 1, []).extend(orders)

    noise_by_height: dict[int, list] = {}
    for nb in noise_bots:
        for h, orders in nb.block_log:
            if h <= after_block:
                continue
            noise_by_height.setdefault(h + 1, []).extend(orders)

    trader_orders_by_height: dict[int, list[tuple[str, list]]] = {}
    for t in traders:
        for h, orders in t.block_log:
            if h <= after_block:
                continue
            trader_orders_by_height.setdefault(h + 1, []).append((t.name, orders))

    records = []
    for height in sorted(all_heights):
        pt = price_by_height.get(height)
        mm_orders = mm_by_height.get(height, [])
        noise_orders = noise_by_height.get(height, [])

        trader_entries = trader_orders_by_height.get(height, [])
        all_trader_orders = []
        for tname, orders in trader_entries:
            all_trader_orders.extend(
                {"trader": tname, "order": _describe_order(o)} for o in orders
            )

        rec = {
            "height": height,
            "timestamp_ms": pt.timestamp_ms if pt else None,
            "sim_time": (
                sim_time_by_height[height].isoformat()
                if sim_time_by_height and height in sim_time_by_height
                else None
            ),
            "yes_price": pt.yes_price_nanos / NANOS_PER_DOLLAR if pt else None,
            "volume_nanos": pt.volume_nanos if pt else 0,
            "mm_orders": [_describe_order(o) for o in mm_orders],
            "noise_orders": [_describe_order(o) for o in noise_orders],
            "noise_order_count": len(noise_orders),
            "trader_orders": [e["order"] for e in all_trader_orders],
            "trader_orders_detail": all_trader_orders,
            "trader_fills": all_trader_fills_by_height.get(height, []),
            "mm_fills": mm_fills_by_height.get(height, []),
            "noise_fills": noise_fills_by_height.get(height, []),
            "trader_llm": llm_by_block.get(height, []),
        }
        records.append(rec)

    # Compute active trader orders with TTL=3 carry-over
    active_orders: list[dict] = []
    for rec in records:
        h = rec["height"]
        active_orders = [o for o in active_orders if h - o["submitted_block"] < 3]
        for o_str in rec["trader_orders"]:
            parts = o_str.split()
            if len(parts) >= 2:
                try:
                    qty = int(parts[1])
                except ValueError:
                    qty = 0
                active_orders.append({"qty": qty, "submitted_block": h})
        rec["active_trader_orders"] = len(active_orders)
        for f in rec["trader_fills"]:
            remaining = f["fill_qty"]
            for o in active_orders:
                if remaining <= 0:
                    break
                if o["qty"] > 0:
                    taken = min(o["qty"], remaining)
                    o["qty"] -= taken
                    remaining -= taken
        active_orders = [o for o in active_orders if o["qty"] > 0]

    return records


async def save_and_print_results(
    client, config, all_bots, traders: list, market_id,
    runs_dir: Path,
    day_label=None, run_id=None, after_block: int = 0,
):
    mm = all_bots[0]
    num_traders = len(traders)
    noise_bots = all_bots[1:-num_traders]

    print("\n" + "=" * 70)
    print("SIMULATION RESULTS")
    print("=" * 70)

    # Leaderboard
    print("\n--- Leaderboard ---")
    print(f"{'Name':<20} {'Balance':>10} {'PosValue':>10} {'Total':>10} {'PnL':>10}")
    print("-" * 62)

    leaderboard = []
    for bot in all_bots:
        try:
            portfolio = await client.get_portfolio(bot.account_id)
            pos_val = portfolio.total_position_value_nanos / NANOS_PER_DOLLAR
            total = portfolio.portfolio_value_nanos / NANOS_PER_DOLLAR
            yes_qty = sum(p.quantity for p in portfolio.positions if p.outcome == "YES")
            no_qty = sum(p.quantity for p in portfolio.positions if p.outcome == "NO")
            leaderboard.append({
                "name": bot.name,
                "account_id": bot.account_id,
                "balance": portfolio.balance_dollars,
                "yes_shares": yes_qty,
                "no_shares": no_qty,
                "position_value": pos_val,
                "portfolio_value": total,
                "pnl": portfolio.pnl_dollars,
            })
        except Exception as e:
            log.warning("Failed to get portfolio for %s: %s", bot.name, e)

    leaderboard.sort(key=lambda r: r["pnl"], reverse=True)
    for r in leaderboard:
        print(
            f"{r['name']:<20} "
            f"${r['balance']:>9.2f} "
            f"${r['position_value']:>9.2f} "
            f"${r['portfolio_value']:>9.2f} "
            f"${r['pnl']:>+9.2f}"
        )

    # Trade logs
    for t in traders:
        day_trade_log = [rec for rec in t.trade_log if rec.block_height > after_block]
        total_articles = sum(len(rec.articles) for rec in day_trade_log)
        print(
            f"\n--- {t.name} Trade Log "
            f"({len(day_trade_log)} decisions, {total_articles} articles) ---"
        )
        for i, rec in enumerate(day_trade_log, 1):
            order_desc = ", ".join(rec.to_dict()["orders"]) or "no trade"
            art_tag = f" ({len(rec.articles)} articles)" if len(rec.articles) > 1 else ""
            print(
                f"  [{i}] {rec.sim_time:%H:%M} FV={rec.fair_value:.2f}{art_tag} "
                f"| {order_desc}"
            )
            for art in rec.articles:
                print(f"       {art.source}: {art.title[:65]}")
            if rec.motivation:
                print(f"       → {rec.motivation[:80]}")

    # Fetch fills
    trader_fills_map: dict[str, list] = {}
    for t in traders:
        trader_fills_map[t.name] = await _fetch_all_fills(client, t.account_id)
    mm_fills = await _fetch_all_fills(client, mm.account_id)
    noise_fills = []
    for nb in noise_bots:
        noise_fills.extend(await _fetch_all_fills(client, nb.account_id))

    # Build per-block records
    price_history = await client.get_price_history(market_id)
    sim_time_by_height: dict[int, datetime] = {}
    for trader in traders:
        for snapshot in trader.price_history:
            if snapshot.block_height > after_block:
                sim_time_by_height.setdefault(snapshot.block_height, snapshot.sim_time)
    block_records = build_block_records(
        all_bots, mm, noise_bots, traders, price_history,
        trader_fills_map=trader_fills_map,
        mm_fills=mm_fills, noise_fills=noise_fills,
        sim_time_by_height=sim_time_by_height,
        after_block=after_block,
    )

    # Enrich with welfare/volume/fills
    # Merge block_stats from all bots (MM may stop early with max_blocks)
    block_stats: dict[int, tuple[int, int, int]] = {}
    for bot in all_bots:
        for h, stats in bot.block_stats.items():
            if h > after_block and h not in block_stats:
                block_stats[h] = stats
    for rec in block_records:
        stats = block_stats.get(rec["height"])
        if stats:
            rec["welfare_nanos"] = stats[0]
            rec["total_volume_nanos"] = stats[1]
            rec["orders_filled"] = stats[2]
        else:
            rec["welfare_nanos"] = 0
            rec["total_volume_nanos"] = 0
            rec["orders_filled"] = 0

    # Block summary
    print(f"\n--- Block Log ({len(block_records)} blocks) ---")
    for rec in block_records:
        price_str = f"YES={rec['yes_price']:.2f}" if rec["yes_price"] is not None else "YES=???"
        mm_n = len(rec["mm_orders"])
        noise_n = rec["noise_order_count"]
        trader_n = len(rec["trader_orders"])
        line = f"  Block {rec['height']:>3}: {price_str}  MM:{mm_n}  Noise:{noise_n}  Trader:{trader_n}"
        for llm in rec["trader_llm"]:
            tag = f"[{llm['trader']}]" if "trader" in llm else ""
            fv = llm.get('fair_value', llm.get('probability', 0))
            line += f"  ← {tag} FV={fv:.2f}"
        print(line)

    # Save to file
    runs_dir.mkdir(parents=True, exist_ok=True)
    run_id = run_id or (
        datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
        + f"-{uuid4().hex}"
    )
    suffix = f"_day{day_label}" if day_label else ""
    run_path = runs_dir / f"{run_id}{suffix}.json"

    run_data = {
        "meta": {
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "simulation_date": day_label,
            "run_id": run_id,
            "config": asdict(config),
        },
        "blocks": block_records,
        "trade_logs": {
            t.name: [
                rec.to_dict()
                for rec in t.trade_log
                if rec.block_height > after_block
            ]
            for t in traders
        },
        "trader_models": {t.name: getattr(t, "model_name", None) for t in traders},
        "leaderboard": leaderboard,
    }
    temporary_path = run_path.with_suffix(f".{uuid4().hex}.tmp")
    try:
        temporary_path.write_text(json.dumps(run_data, indent=2, default=str))
        os.replace(temporary_path, run_path)
    finally:
        temporary_path.unlink(missing_ok=True)
    print(f"\nResults saved to {run_path}")
    return run_path
